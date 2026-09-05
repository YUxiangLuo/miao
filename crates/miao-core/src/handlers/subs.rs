use axum::{extract::State, http::StatusCode, response::Json};
use std::collections::{HashMap, HashSet};
use std::sync::{atomic::Ordering, Arc};

use crate::models::{
    ApiResponse, DisabledNode, SetNodeDisabledRequest, SubBatchRequest, SubBatchResult,
    SubNodeInfo, SubNodesInfo, SubRequest, SubStatus, SubscriptionState, VergeImportItem,
    VergeImportResult,
};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::{
    apply_disabled_nodes, collect_manual_outbounds, edit_subscriptions, read_sub_nodes_snapshot,
    refresh_subscriptions_foreground, subscription_source_id, ConfigMutationError,
};
use crate::services::subscription::is_informational_subscription_node;
use crate::services::verge;
use crate::state::AppState;
use crate::validation::Validator;

pub async fn get_subs(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<SubStatus>>> {
    // 快照磁盘读先于一切锁：本 handler 是面板轮询热点，不把 IO 关进临界区
    let snapshot = read_sub_nodes_snapshot(&state).await;
    let config = state.config.read().await;
    let status_map = state.sub_status.lock().await;
    // disabled_count 用生效口径：只统计匹配当前快照节点的条目；
    // 失配条目（如机场的「剩余流量」信息节点改名后）不产生效果，不应计入

    let source_by_url: HashMap<&str, String> = config
        .subs
        .iter()
        .map(|url| (url.as_str(), subscription_source_id(url)))
        .collect();
    let mut disabled_counts: HashMap<&str, usize> = HashMap::new();
    for entry in &config.disabled_nodes {
        if source_by_url
            .get(entry.sub.as_str())
            .and_then(|source| snapshot.as_ref()?.live_names.get(source))
            .is_some_and(|names| names.contains(&entry.name))
        {
            *disabled_counts.entry(&entry.sub).or_default() += 1;
        }
    }
    let subs_with_status: Vec<SubStatus> = config
        .subs
        .iter()
        .map(|url| {
            let mut status = status_map.get(url).cloned().unwrap_or(SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                disabled_count: 0,
                state: SubscriptionState::Pending,
                error: None,
            });
            status.disabled_count = disabled_counts.get(url.as_str()).copied().unwrap_or(0);
            status
        })
        .collect();

    success("Subscriptions loaded", subs_with_status)
}

/// 订阅详情弹窗的数据源：按订阅分组列出节点（含禁用标记）。
/// 纯读路径：数据来自 sub-nodes.json 快照，零网络；快照缺失的订阅返回空列表。
pub async fn get_sub_nodes(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<SubNodesInfo>>> {
    let config = state.config.read().await;
    let snapshot = read_sub_nodes_snapshot(&state).await;

    let disabled_keys: HashSet<(String, &str)> = config
        .disabled_nodes
        .iter()
        .map(|entry| (subscription_source_id(&entry.sub), entry.name.as_str()))
        .collect();

    let groups = config
        .subs
        .iter()
        .map(|url| {
            let source_id = subscription_source_id(url);
            let mut nodes = Vec::new();
            if let Some(snapshot) = &snapshot {
                for (index, entry_source) in snapshot.source_ids.iter().enumerate() {
                    if *entry_source != source_id {
                        continue;
                    }
                    let (Some(name), Some(outbound)) = (
                        snapshot.node_names.get(index),
                        snapshot.outbounds.get(index),
                    ) else {
                        continue;
                    };
                    if is_informational_subscription_node(name, outbound) {
                        continue;
                    }
                    let str_field = |key: &str| {
                        outbound
                            .get(key)
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                    };
                    nodes.push(SubNodeInfo {
                        name: name.clone(),
                        server: str_field("server").to_string(),
                        server_port: outbound
                            .get("server_port")
                            .and_then(|value| value.as_u64())
                            .unwrap_or_default() as u16,
                        node_type: str_field("type").to_string(),
                        disabled: disabled_keys.contains(&(source_id.clone(), name.as_str())),
                    });
                }
            }
            // 失配的禁用条目：存在禁用记录但当前快照里已没有同名节点
            let stale_disabled = config
                .disabled_nodes
                .iter()
                .filter(|entry| subscription_source_id(&entry.sub) == source_id)
                .filter(|entry| !nodes.iter().any(|node| node.name == entry.name))
                .map(|entry| entry.name.clone())
                .collect();
            SubNodesInfo {
                url: url.clone(),
                nodes,
                stale_disabled,
            }
        })
        .collect();

    success("Subscription nodes loaded", groups)
}

/// 禁用/启用订阅节点（易变层）。本地语义变更：快照零网络重建 + 热应用。
/// 成功路径会热重启内核；校验失败（订阅/节点不存在、禁用后空池）在事务前拦截。
pub async fn set_node_disabled(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetNodeDisabledRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let old_config = state.config.read().await.clone();
    // 锁外咨询性校验（快速 400 + 精确报错）；订阅存在性与空池校验在锁内闭包里
    // 还会基于最新配置权威重查——上锁间隙的并发变更不会逃过校验
    if !old_config.subs.contains(&req.sub) {
        return Err(status_error(StatusCode::BAD_REQUEST, "订阅不存在"));
    }
    // 存在性校验只约束「禁用」；「启用/清理」按名字移除条目即可，
    // 必须能清理已失配的条目（节点改名后快照里已不存在）
    let mut entries: Vec<(String, String)> = Vec::new();
    if req.disabled {
        let Some(snapshot) = read_sub_nodes_snapshot(&state).await else {
            return Err(status_error(
                StatusCode::BAD_REQUEST,
                "订阅节点尚未获取，无法设置禁用",
            ));
        };
        entries = snapshot
            .source_ids
            .iter()
            .zip(snapshot.node_names.iter())
            .enumerate()
            .filter_map(|(index, (source, name))| {
                let outbound = snapshot.outbounds.get(index)?;
                (!is_informational_subscription_node(name, outbound))
                    .then_some((source.clone(), name.clone()))
            })
            .collect();
        let source_id = subscription_source_id(&req.sub);
        if !entries.contains(&(source_id, req.name.clone())) {
            return Err(status_error(StatusCode::BAD_REQUEST, "订阅中不存在该节点"));
        }
    }

    let sub = req.sub.clone();
    let name = req.name.clone();
    let disabled = req.disabled;
    let result = apply_disabled_nodes(&state, move |config| {
        if !config.subs.contains(&sub) {
            return Err("订阅不存在".to_string());
        }
        let mut next = config.disabled_nodes.clone();
        if disabled {
            if !next
                .iter()
                .any(|entry| entry.sub == sub && entry.name == name)
            {
                next.push(DisabledNode {
                    sub: sub.clone(),
                    name: name.clone(),
                });
            }
            // selector/urltest 不允许空 outbounds；禁用后可用池（订阅剩余 + 有效手动节点）
            // 为空会生成非法配置。锁内基于最新禁用集计算：并发禁用请求不会叠加出空池
            let disabled_keys: HashSet<(String, &str)> = next
                .iter()
                .map(|entry| (subscription_source_id(&entry.sub), entry.name.as_str()))
                .collect();
            let remaining = entries
                .iter()
                .filter(|(source, node_name)| {
                    !disabled_keys.contains(&(source.clone(), node_name.as_str()))
                })
                .count();
            let manual = collect_manual_outbounds(config).0.len();
            if remaining + manual == 0 {
                return Err("不能禁用全部节点".to_string());
            }
        } else {
            next.retain(|entry| !(entry.sub == sub && entry.name == name));
        }
        config.disabled_nodes = next;
        Ok(())
    })
    .await;

    match result {
        Ok(update) => Ok(success_no_data(if update.updated() {
            if req.disabled {
                "节点已禁用"
            } else {
                "节点已启用"
            }
        } else {
            "节点状态未变化"
        })),
        Err(ConfigMutationError::Superseded) => {
            Err(status_error(StatusCode::CONFLICT, "操作已被更新的请求取代"))
        }
        Err(ConfigMutationError::Rejected(message)) => {
            Err(status_error(StatusCode::BAD_REQUEST, message))
        }
        Err(ConfigMutationError::Apply(e)) => {
            Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e))
        }
    }
}

pub async fn add_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    if let Err(e) = Validator::subscription_url(&req.url) {
        return Err(status_error(StatusCode::BAD_REQUEST, e));
    }

    edit_subscriptions(&state, false, |subs| {
        if subs.contains(&req.url) {
            return Err("Subscription already exists".to_string());
        }
        subs.push(req.url);
        Ok(())
    })
    .await
    .map_err(|error| subscription_error(error, StatusCode::BAD_REQUEST))?;
    Ok(success_no_data("Subscription added"))
}

/// 扫描本机 clash-verge-rev 的订阅（只读）。未安装/无 remote 订阅时 found=false。
pub async fn get_verge_import(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<VergeImportResult>> {
    let subs = verge::scan().await.unwrap_or_default();
    let config = state.config.read().await;
    let items: Vec<VergeImportItem> = subs
        .into_iter()
        .map(|sub| VergeImportItem {
            already_added: config.subs.contains(&sub.url),
            name: sub.name,
            url: sub.url,
        })
        .collect();
    success(
        "Clash Verge subscriptions scanned",
        VergeImportResult {
            found: !items.is_empty(),
            items,
        },
    )
}

/// 批量添加订阅：与逐条调 add_sub 不同，全部 URL 在一次配置事务内提交，
/// 只触发一次生成/校验/热重载。已存在或批内重复的跳过并计数。
pub async fn add_subs_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubBatchRequest>,
) -> HandlerResult<SubBatchResult> {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let mut urls: Vec<String> = Vec::with_capacity(req.urls.len());
    for raw in &req.urls {
        let url = raw.trim().to_string();
        if let Err(e) = Validator::subscription_url(&url) {
            return Err(status_error(StatusCode::BAD_REQUEST, e));
        }
        if !urls.contains(&url) {
            urls.push(url);
        }
    }

    let (result, _) = edit_subscriptions(&state, false, |subs| {
        let mut added = 0;
        let mut skipped = 0;
        for url in urls {
            if subs.contains(&url) {
                skipped += 1;
            } else {
                subs.push(url);
                added += 1;
            }
        }
        Ok(SubBatchResult { added, skipped })
    })
    .await
    .map_err(|error| subscription_error(error, StatusCode::BAD_REQUEST))?;
    Ok(success(
        if result.added == 0 {
            "No new subscriptions to add"
        } else {
            "Subscriptions added"
        },
        result,
    ))
}

pub async fn delete_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    edit_subscriptions(&state, false, |subs| {
        let previous = subs.len();
        subs.retain(|url| url != &req.url);
        if previous == subs.len() {
            return Err("Subscription not found".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|error| subscription_error(error, StatusCode::NOT_FOUND))?;
    Ok(success_no_data("Subscription deleted"))
}

pub async fn refresh_subs(State(state): State<Arc<AppState>>) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let update = refresh_subscriptions_foreground(&state)
        .await
        .map_err(|error| subscription_error(error, StatusCode::BAD_REQUEST))?;
    Ok(success_no_data(if update.updated() {
        "Subscriptions refreshed and runtime updated"
    } else {
        "Subscriptions refreshed"
    }))
}

fn subscription_error<T: serde::Serialize>(
    error: ConfigMutationError,
    rejected: StatusCode,
) -> (StatusCode, Json<ApiResponse<T>>) {
    let status = match &error {
        ConfigMutationError::Rejected(_) => rejected,
        ConfigMutationError::Superseded => StatusCode::CONFLICT,
        ConfigMutationError::Apply(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    status_error(status, error)
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, response::Json};

    use super::{add_subs_batch, get_sub_nodes, get_subs, get_verge_import, set_node_disabled};
    use crate::{
        error::AppError,
        models::{
            Config, DisabledNode, SetNodeDisabledRequest, SubBatchRequest, SubscriptionState,
        },
        services::config::{save_sub_nodes_snapshot, subscription_source_id, SubNodesSnapshot},
        test_support::app_state,
    };

    fn config_with_subs(subs: &[&str]) -> Config {
        Config {
            port: None,
            subs: subs.iter().map(|s| s.to_string()).collect(),
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        }
    }

    /// 构造 sub-nodes.json 快照：nodes 为 (订阅 URL, 节点名) 列表
    fn snapshot_for(subs: &[&str], nodes: &[(&str, &str)]) -> SubNodesSnapshot {
        SubNodesSnapshot {
            version: 1,
            subs: subs.iter().map(|s| s.to_string()).collect(),
            node_names: nodes.iter().map(|(_, name)| name.to_string()).collect(),
            outbounds: nodes
                .iter()
                .map(|_| {
                    serde_json::json!({
                        "type": "trojan",
                        "server": "example.com",
                        "server_port": 443,
                    })
                })
                .collect(),
            source_ids: nodes
                .iter()
                .map(|(sub, _)| subscription_source_id(sub))
                .collect(),
        }
    }

    fn disabled_entry(sub: &str, name: &str) -> DisabledNode {
        DisabledNode {
            sub: sub.to_string(),
            name: name.to_string(),
        }
    }

    fn disable_request(sub: &str, name: &str, disabled: bool) -> Json<SetNodeDisabledRequest> {
        Json(SetNodeDisabledRequest {
            sub: sub.to_string(),
            name: name.to_string(),
            disabled,
        })
    }

    // app_state 默认 initializing=true（写路径的 409 闸），handler 直测需手动关闸
    fn ready_state(config: Config) -> std::sync::Arc<crate::state::AppState> {
        let state = app_state(config);
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        state
    }

    #[test]
    fn app_error_context_message_stays_user_visible() {
        let err = AppError::context(
            "Failed to apply config change; rolled back to previous config",
            AppError::message("new config invalid"),
        );

        assert_eq!(
            err.to_string(),
            "Failed to apply config change; rolled back to previous config: new config invalid"
        );
    }

    #[tokio::test]
    async fn get_subs_returns_default_pending_status_when_status_missing() {
        let state = app_state(config_with_subs(&["https://example.com/sub"]));

        let Json(response) = get_subs(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "Subscriptions loaded");
        let subs = response.data.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].url, "https://example.com/sub");
        assert!(!subs[0].success);
        assert_eq!(subs[0].state, SubscriptionState::Pending);
        assert_eq!(subs[0].node_count, 0);
        assert!(subs[0].error.is_none());
    }

    #[tokio::test]
    async fn get_subs_reports_effective_disabled_count() {
        // 生效口径：只有匹配当前快照节点的条目才计数，失配条目不产生效果不计入
        let subs = ["https://example.com/a", "https://example.com/b"];
        let mut config = config_with_subs(&subs);
        config.disabled_nodes = vec![
            disabled_entry("https://example.com/a", "node-1"),
            disabled_entry("https://example.com/a", "node-2"),
            disabled_entry("https://example.com/a", "剩余流量：45 GB"), // 信息项即使匹配快照也不计入
        ];
        let state = app_state(config);
        let snapshot = snapshot_for(
            &subs,
            &[
                ("https://example.com/a", "node-1"),
                ("https://example.com/a", "node-2"),
                ("https://example.com/a", "剩余流量：45 GB"), // 自动过滤的信息项
            ],
        );
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let Json(response) = get_subs(State(state)).await;

        let subs_status = response.data.unwrap();
        assert_eq!(subs_status[0].disabled_count, 2);
        assert_eq!(subs_status[1].disabled_count, 0);
    }

    #[tokio::test]
    async fn get_subs_counts_zero_disabled_without_snapshot() {
        let mut config = config_with_subs(&["https://example.com/a"]);
        config.disabled_nodes = vec![disabled_entry("https://example.com/a", "node-1")];
        let state = app_state(config);

        let Json(response) = get_subs(State(state)).await;

        assert_eq!(response.data.unwrap()[0].disabled_count, 0);
    }

    #[tokio::test]
    async fn get_sub_nodes_groups_nodes_with_disabled_flags() {
        let subs = ["https://example.com/a", "https://example.com/b"];
        let mut config = config_with_subs(&subs);
        config.disabled_nodes = vec![disabled_entry("https://example.com/a", "node-1")];
        let state = app_state(config);
        let snapshot = snapshot_for(
            &subs,
            &[
                ("https://example.com/a", "node-1"),
                ("https://example.com/a", "node-2"),
                ("https://example.com/b", "node-1"),
            ],
        );
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let Json(response) = get_sub_nodes(State(state)).await;

        assert!(response.success);
        let groups = response.data.unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].url, "https://example.com/a");
        assert_eq!(groups[0].nodes.len(), 2);
        // a 订阅的 node-1 被禁用；同名禁用不波及 b 订阅的同名节点
        assert!(groups[0].nodes[0].disabled);
        assert!(!groups[0].nodes[1].disabled);
        assert!(!groups[1].nodes[0].disabled);
        assert_eq!(groups[0].nodes[0].server, "example.com");
        assert_eq!(groups[0].nodes[0].server_port, 443);
        assert_eq!(groups[0].nodes[0].node_type, "trojan");
        assert!(groups[0].stale_disabled.is_empty());
    }

    #[tokio::test]
    async fn get_sub_nodes_reports_stale_disabled_entries() {
        let subs = ["https://example.com/a"];
        let mut config = config_with_subs(&subs);
        config.disabled_nodes = vec![
            disabled_entry("https://example.com/a", "node-1"), // 生效中
            disabled_entry("https://example.com/a", "已改名的节点"), // 失配
        ];
        let state = app_state(config);
        let snapshot = snapshot_for(&subs, &[("https://example.com/a", "node-1")]);
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let Json(response) = get_sub_nodes(State(state)).await;

        let groups = response.data.unwrap();
        assert!(groups[0].nodes[0].disabled);
        assert_eq!(groups[0].stale_disabled, vec!["已改名的节点".to_string()]);
    }

    #[tokio::test]
    async fn get_sub_nodes_hides_informational_entries_from_old_snapshots() {
        let subs = ["https://example.com/a"];
        let state = app_state(config_with_subs(&subs));
        let snapshot = snapshot_for(
            &subs,
            &[
                ("https://example.com/a", "剩余流量：19.06 GB"),
                ("https://example.com/a", "香港 01"),
            ],
        );
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let Json(response) = get_sub_nodes(State(state)).await;

        let groups = response.data.unwrap();
        assert_eq!(groups[0].nodes.len(), 1);
        assert_eq!(groups[0].nodes[0].name, "香港 01");
    }

    #[tokio::test]
    async fn get_sub_nodes_returns_empty_nodes_without_snapshot() {
        let state = app_state(config_with_subs(&["https://example.com/a"]));

        let Json(response) = get_sub_nodes(State(state)).await;

        let groups = response.data.unwrap();
        assert_eq!(groups.len(), 1);
        assert!(groups[0].nodes.is_empty());
    }

    #[tokio::test]
    async fn set_node_disabled_rejects_unknown_subscription() {
        let state = ready_state(config_with_subs(&["https://example.com/a"]));

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/nope", "node-1", true),
        )
        .await;

        let Err((status, _)) = result else {
            panic!("unknown subscription must be rejected")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_node_disabled_requires_fetched_snapshot() {
        let state = ready_state(config_with_subs(&["https://example.com/a"]));

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/a", "node-1", true),
        )
        .await;

        let Err((status, Json(body))) = result else {
            panic!("missing snapshot must be rejected")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.message.contains("尚未获取"));
    }

    #[tokio::test]
    async fn set_node_disabled_rejects_unknown_node() {
        let subs = ["https://example.com/a"];
        let state = ready_state(config_with_subs(&subs));
        let snapshot = snapshot_for(&subs, &[("https://example.com/a", "node-1")]);
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/a", "nope", true),
        )
        .await;

        let Err((status, _)) = result else {
            panic!("unknown node must be rejected")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_node_disabled_rejects_filtered_informational_entry() {
        let subs = ["https://example.com/a"];
        let state = ready_state(config_with_subs(&subs));
        let snapshot = snapshot_for(
            &subs,
            &[
                ("https://example.com/a", "官网 example.com"),
                ("https://example.com/a", "香港 01"),
            ],
        );
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/a", "官网 example.com", true),
        )
        .await;

        let Err((status, _)) = result else {
            panic!("informational entries must be removed before manual disabling")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_node_disabled_rejects_emptying_the_pool() {
        // 唯一的订阅节点被禁用后节点池为空 → 生成非法 selector，必须在事务前拦住
        let subs = ["https://example.com/a"];
        let state = ready_state(config_with_subs(&subs));
        let snapshot = snapshot_for(&subs, &[("https://example.com/a", "node-1")]);
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/a", "node-1", true),
        )
        .await;

        let Err((status, Json(body))) = result else {
            panic!("disabling the last node must be rejected")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.message.contains("不能禁用全部节点"));
    }

    #[tokio::test]
    async fn set_node_disabled_allows_clearing_stale_entry() {
        // 启用/清理不校验节点存在：失配条目（节点改名后）必须能清掉。
        // 条目不存在时 retain 无效果 → 幂等返回「未变化」，不触碰内核
        let subs = ["https://example.com/a"];
        let state = ready_state(config_with_subs(&subs));

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/a", "已改名的节点", false),
        )
        .await;

        let Ok(Json(response)) = result else {
            panic!("clearing a stale entry must not be rejected")
        };
        assert!(response.success);
        assert_eq!(response.message, "节点状态未变化");
    }

    #[tokio::test]
    async fn set_node_disabled_disallow_missing_sub_even_when_enabling() {
        let state = ready_state(config_with_subs(&["https://example.com/a"]));

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/nope", "node-1", false),
        )
        .await;

        let Err((status, _)) = result else {
            panic!("unknown subscription must be rejected even when enabling")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn set_node_disabled_allows_disabling_when_manual_nodes_remain() {
        // 有手动节点时禁用唯一订阅节点不会空池——但成功路径会热应用起内核，
        // 单测只验证它不被空池校验误拦（到 apply 前的错误一律是校验错误）
        let subs = ["https://example.com/a"];
        let mut config = config_with_subs(&subs);
        config.nodes = vec![
            r#"{"type":"trojan","tag":"manual","server":"m.example.com","server_port":443,"password":"x"}"#
                .to_string(),
        ];
        let state = ready_state(config);
        let snapshot = snapshot_for(&subs, &[("https://example.com/a", "node-1")]);
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();

        let result = set_node_disabled(
            State(state),
            disable_request("https://example.com/a", "node-1", true),
        )
        .await;

        // 不被 400 拦截即通过校验；apply 在测试环境失败（无内核）是可接受的
        if let Err((status, _)) = result {
            assert_ne!(status, StatusCode::BAD_REQUEST);
        }
    }

    // 扫描依赖真实路径解析；开发与 CI 机器上都没有 clash-verge-rev → found=false。
    #[tokio::test]
    async fn verge_import_reports_not_found_without_verge_install() {
        let state = app_state(config_with_subs(&[]));

        let Json(response) = get_verge_import(State(state)).await;

        assert!(response.success);
        let result = response.data.unwrap();
        assert!(!result.found);
        assert!(result.items.is_empty());
    }

    #[tokio::test]
    async fn batch_rejects_invalid_urls_before_touching_config() {
        let state = ready_state(config_with_subs(&[]));

        let result = add_subs_batch(
            State(state),
            Json(SubBatchRequest {
                urls: vec!["not-a-url".to_string()],
            }),
        )
        .await;

        let Err((status, _)) = result else {
            panic!("invalid URL must be rejected")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // 全部重复时不进配置事务（不起内核、不落盘），直接计数返回。
    #[tokio::test]
    async fn batch_skips_existing_without_applying() {
        let state = ready_state(config_with_subs(&["https://example.com/sub"]));

        let result = add_subs_batch(
            State(state.clone()),
            Json(SubBatchRequest {
                urls: vec![
                    "https://example.com/sub".to_string(),
                    " https://example.com/sub ".to_string(), // 批内重复（先去重）
                ],
            }),
        )
        .await;

        let Ok(Json(response)) = result else {
            panic!("all-duplicate batch must succeed without applying")
        };
        assert!(response.success);
        let result = response.data.unwrap();
        assert_eq!(result.added, 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(state.config.read().await.subs.len(), 1);
    }
}
