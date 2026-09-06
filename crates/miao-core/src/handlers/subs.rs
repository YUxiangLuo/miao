use crate::models::{
    ApiResponse, SetNodeDisabledRequest, SubBatchRequest, SubBatchResult, SubNodesInfo, SubRequest,
    SubStatus, VergeImportResult,
};
use crate::responses::{command_reply, command_result, HandlerResult};
use crate::services::commands;
use crate::state::AppState;
use axum::{extract::State, response::Json};
use std::sync::Arc;

pub async fn get_subs(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<SubStatus>>> {
    command_reply(commands::subs::get_subs(state).await)
}

pub async fn get_sub_nodes(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<SubNodesInfo>>> {
    command_reply(commands::subs::get_sub_nodes(state).await)
}

pub async fn set_node_disabled(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetNodeDisabledRequest>,
) -> HandlerResult {
    command_result(commands::subs::set_node_disabled(state, req).await)
}

pub async fn add_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    command_result(commands::subs::add_sub(state, req).await)
}

pub async fn get_verge_import(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<VergeImportResult>> {
    command_reply(commands::subs::get_verge_import(state).await)
}

pub async fn add_subs_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubBatchRequest>,
) -> HandlerResult<SubBatchResult> {
    command_result(commands::subs::add_subs_batch(state, req).await)
}

pub async fn delete_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    command_result(commands::subs::delete_sub(state, req).await)
}

pub async fn refresh_subs(State(state): State<Arc<AppState>>) -> HandlerResult {
    command_result(commands::subs::refresh_subs(state).await)
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
