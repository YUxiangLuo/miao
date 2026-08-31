use futures::{stream, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::{atomic::Ordering, Arc};
use tokio::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::error::AppResult;
use crate::models::{
    node_multiplier, Config, DisabledNode, NodeMultiplier, NodeSelect, SubStatus, SubscriptionState,
};
use crate::services::subscription::{fetch_sub, is_informational_subscription_node};
use crate::state::{AppState, SkippedRule};

use super::bindings::{assign_subscription_tags, reserved_node_tags, NodeTagBindings};
use super::builder::build_sing_box_config_with_multipliers;
use super::persist::{read_sub_nodes_snapshot, save_sub_nodes_snapshot, SubNodesSnapshot};

#[derive(Clone)]
pub struct GenConfigOutcome {
    pub bytes: Vec<u8>,
    pub has_sub_nodes: bool,
    pub node_select: NodeSelect,
    pub skipped_rules: Vec<SkippedRule>,
    /// 完整可用节点池中识别出的倍率，供面板动态生成自动候选上限选项。
    pub available_multipliers: Vec<NodeMultiplier>,
    /// 本次真拉取拿到的订阅节点集（非空才 Some）；快照重建为 None。
    /// 校验通过/启动成功后由 record_fresh_snapshot 落盘，供本地语义变更零网络重建。
    pub fresh_sub_nodes: Option<Vec<FetchedNode>>,
    pub node_bindings: NodeTagBindings,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FetchedNode {
    pub source_id: String,
    pub name: String,
    pub outbound: serde_json::Value,
}

pub fn subscription_source_id(url: &str) -> String {
    hex::encode(Sha256::digest(url.as_bytes()))
}

const MAX_CONCURRENT_SUBS: usize = 5;

/// 订阅全失败时的退避重试预算。
/// 开机竞速（miao 先于默认路由/DHCP 就绪启动）时订阅请求会全部秒败；
/// 预算内的退避重试可以跨过这个窗口。手动刷新传 None：用户在场，失败即报。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubFetchRetry {
    /// 不重试
    #[default]
    None,
    /// 启动路径预算：见 STARTUP_RETRY_SCHEDULE
    Startup,
}

/// 启动刷新拥有一个绝对截止时间。快速失败时仍通过短退避跨过 DHCP/默认路由
/// 尚未就绪的窗口；单个卡死请求和订阅数量都不能把总等待无限放大。
const STARTUP_FETCH_BUDGET: Duration = Duration::from_secs(20);
const STARTUP_RETRY_SCHEDULE: &[Duration] = &[
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];

/// 测试里缩短以保持用例快速（与 singbox.rs 的 KERNEL_WATCH_INTERVAL 同款手法）
#[cfg(test)]
const STARTUP_RETRY_SCHEDULE_TEST: &[Duration] =
    &[Duration::from_millis(50), Duration::from_millis(100)];

#[cfg(test)]
const STARTUP_FETCH_BUDGET_TEST: Duration = Duration::from_secs(2);

#[cfg(not(test))]
fn startup_fetch_budget() -> Duration {
    STARTUP_FETCH_BUDGET
}

#[cfg(test)]
fn startup_fetch_budget() -> Duration {
    STARTUP_FETCH_BUDGET_TEST
}

#[cfg(not(test))]
fn startup_retry_schedule() -> &'static [Duration] {
    STARTUP_RETRY_SCHEDULE
}

#[cfg(test)]
fn startup_retry_schedule() -> &'static [Duration] {
    STARTUP_RETRY_SCHEDULE_TEST
}

impl SubFetchRetry {
    fn schedule(self) -> &'static [Duration] {
        match self {
            Self::None => &[],
            Self::Startup => startup_retry_schedule(),
        }
    }
}

/// 拉取订阅并写出 sing-box 配置；订阅全失败时按 retry 预算退避重试。
/// 返回是否拿到订阅节点，以及实际生效的 node_select。
pub async fn gen_config(
    config: &Config,
    state: &Arc<AppState>,
    retry: SubFetchRetry,
) -> AppResult<GenConfigOutcome> {
    let nodes = fetch_sub_nodes(config, state, retry).await;
    gen_config_from_nodes(config, state, nodes).await
}

/// 只拉取订阅节点集（不写盘）：订阅全失败时按 retry 预算退避重试。
/// 供「先拉取、后持锁落地」的调用方（启动后台刷新）把网络等待移出配置锁。
async fn fetch_sub_nodes(
    config: &Config,
    state: &Arc<AppState>,
    retry: SubFetchRetry,
) -> Vec<FetchedNode> {
    fetch_sub_nodes_inner(config, state, retry, None).await
}

/// Startup background work uses an optimistic generation while network I/O is
/// outside the config lock. Once a foreground subscription operation advances
/// that generation, the stale fetch stops publishing per-subscription status
/// and its caller will discard the fetched nodes as well.
pub async fn fetch_sub_nodes_if_current(
    config: &Config,
    state: &Arc<AppState>,
    retry: SubFetchRetry,
    expected_generation: u64,
) -> Vec<FetchedNode> {
    fetch_sub_nodes_inner(config, state, retry, Some(expected_generation)).await
}

fn refresh_generation_is_current(state: &AppState, expected_generation: Option<u64>) -> bool {
    expected_generation
        .is_none_or(|expected| state.sub_refresh_generation.load(Ordering::Relaxed) == expected)
}

async fn fetch_sub_nodes_inner(
    config: &Config,
    state: &Arc<AppState>,
    retry: SubFetchRetry,
    expected_generation: Option<u64>,
) -> Vec<FetchedNode> {
    let schedule = retry.schedule();
    let deadline =
        matches!(retry, SubFetchRetry::Startup).then(|| Instant::now() + startup_fetch_budget());
    let mut attempt = 0usize;
    loop {
        if !refresh_generation_is_current(state, expected_generation) {
            return Vec::new();
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            warn!("Startup subscription refresh budget exhausted");
            return Vec::new();
        }

        let nodes = fetch_all_subs(config, state, deadline, expected_generation).await;
        if !refresh_generation_is_current(state, expected_generation) {
            return Vec::new();
        }
        // 是否值得退避重试：配置了订阅却一个订阅节点都没拿到才算
        // （全部秒败是网络未就绪的典型瞬态）；部分成功/其他错误更像订阅本身坏了
        if !nodes.is_empty() || config.subs.is_empty() {
            return nodes;
        }
        let Some(delay) = schedule.get(attempt) else {
            return nodes;
        };
        if deadline.is_some_and(|deadline| Instant::now() + *delay >= deadline) {
            warn!("Startup subscription refresh budget exhausted before next retry");
            return nodes;
        }
        attempt += 1;
        info!(
            delay_ms = delay.as_millis(),
            attempt, "All subscriptions failed; retrying after backoff"
        );
        tokio::time::sleep(*delay).await;
    }
}

/// 用已获取的订阅节点集构建并写出配置：gen_config 与预拉取（Prefetched）共用。
pub async fn gen_config_from_nodes(
    config: &Config,
    state: &Arc<AppState>,
    nodes: Vec<FetchedNode>,
) -> AppResult<GenConfigOutcome> {
    let fresh = (!nodes.is_empty()).then(|| nodes.clone());
    let mut outcome = build_prepared(config, state, nodes).await?;
    outcome.fresh_sub_nodes = fresh;
    Ok(outcome)
}

/// 用订阅节点集快照零网络重建配置；快照缺失或与当前订阅列表不匹配时退化到真拉取。
/// 本地语义变更（节点选择/路由模式/规则/手动节点）走这里：
/// 切换不是刷新，不该被订阅网络故障拖累。
pub async fn gen_config_from_snapshot(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<GenConfigOutcome> {
    if let Some(snapshot) = read_sub_nodes_snapshot(state).await {
        if snapshot.matches_subs(&config.subs) {
            info!("Rebuilding config from subscription node snapshot (no network)");
            return build_prepared(config, state, snapshot.into_fetched_nodes()).await;
        }
        warn!("Subscription list changed since snapshot; fetching subscriptions");
    }
    gen_config(config, state, SubFetchRetry::None).await
}

/// 校验通过/启动成功后调用：把本次真拉取的节点集落成快照（best-effort，写失败只告警）。
pub async fn record_fresh_snapshot(
    config: &Config,
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
) {
    let Some(nodes) = &outcome.fresh_sub_nodes else {
        return;
    };
    let snapshot = SubNodesSnapshot::from_fetched_nodes(config.subs.clone(), nodes.clone());
    if let Err(err) = save_sub_nodes_snapshot(state, &snapshot).await {
        warn!(error = %err, "Failed to save subscription nodes snapshot");
    }
}

/// 并发拉取全部订阅并逐条更新 sub_status；返回合并后的节点名与 outbounds。
async fn fetch_all_subs(
    config: &Config,
    state: &Arc<AppState>,
    deadline: Option<Instant>,
    expected_generation: Option<u64>,
) -> Vec<FetchedNode> {
    let mut final_nodes = vec![];

    {
        let mut status_map = state.sub_status.lock().await;
        // Re-check after acquiring the status lock. A foreground refresh may
        // have advanced the generation while this task was waiting for it.
        if !refresh_generation_is_current(state, expected_generation) {
            return final_nodes;
        }
        status_map.retain(|url, _| config.subs.contains(url));
        for url in &config.subs {
            let status = status_map.entry(url.clone()).or_insert_with(|| SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                disabled_count: 0,
                state: SubscriptionState::Pending,
                error: None,
            });
            status.state = SubscriptionState::Refreshing;
        }
    }

    let sub_futures: Vec<_> = config
        .subs
        .iter()
        .map(|sub| {
            let sub = sub.clone();
            let client = state.http_client.clone();
            async move {
                info!(url = %sub, "Fetching subscription");
                let request_budget = deadline
                    .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                    .unwrap_or(Duration::from_secs(30))
                    .min(Duration::from_secs(30));
                let result = tokio::time::timeout(request_budget, fetch_sub(&sub, &client)).await;

                match result {
                    Ok(Ok(fetch_result)) => {
                        let valid_count = fetch_result.node_names.len();
                        let total_count = fetch_result.total_count;
                        let error_count = fetch_result.parse_errors.len();
                        let filtered_info_count = fetch_result.filtered_info_count;

                        if filtered_info_count > 0 {
                            info!(
                                url = %sub,
                                filtered = filtered_info_count,
                                "Filtered informational subscription entries"
                            );
                        }
                        if error_count > 0 {
                            warn!(
                                url = %sub,
                                valid = valid_count,
                                total = total_count,
                                errors = error_count,
                                "Partial fetch: some nodes failed to parse"
                            );
                        } else {
                            info!(
                                url = %sub,
                                nodes = valid_count,
                                "Subscription fetched successfully"
                            );
                        }

                        (sub.clone(), Ok(fetch_result))
                    }
                    Ok(Err(e)) => {
                        error!(url = %sub, error = %e, "Failed to fetch subscription");
                        (sub.clone(), Err(e.to_string()))
                    }
                    Err(_) => {
                        error!(url = %sub, timeout_ms = request_budget.as_millis(), "Subscription fetch timed out");
                        let message = if deadline.is_some() {
                            "Startup refresh budget exhausted"
                        } else {
                            "Request timeout"
                        };
                        (sub.clone(), Err(message.to_string()))
                    }
                }
            }
        })
        .collect();

    // 使用 buffer_unordered 限制并发数，避免同时发起过多请求
    let mut results: Vec<_> = stream::iter(sub_futures)
        .buffer_unordered(MAX_CONCURRENT_SUBS)
        .collect()
        .await;

    // 按原始顺序排序结果
    let subs_order: Vec<String> = config.subs.clone();
    results.sort_by_key(|(url, _)| {
        subs_order
            .iter()
            .position(|s| s == url)
            .unwrap_or(usize::MAX)
    });

    for (url, result) in results {
        if !refresh_generation_is_current(state, expected_generation) {
            break;
        }
        let status = match result {
            Ok(fetch_result) => {
                let count = fetch_result.node_names.len();
                let filtered_info_count = fetch_result.filtered_info_count;
                let source_id = subscription_source_id(&url);
                final_nodes.extend(
                    fetch_result
                        .node_names
                        .into_iter()
                        .zip(fetch_result.outbounds)
                        .map(|(name, outbound)| FetchedNode {
                            source_id: source_id.clone(),
                            name,
                            outbound,
                        }),
                );

                let error_info = if !fetch_result.parse_errors.is_empty() {
                    Some(format!(
                        "{} nodes skipped due to parse errors",
                        fetch_result.parse_errors.len()
                    ))
                } else if count == 0 && filtered_info_count > 0 {
                    Some(format!(
                        "No proxy nodes found ({} informational entries filtered)",
                        filtered_info_count
                    ))
                } else if count == 0 && fetch_result.total_count > 0 {
                    Some("All nodes invalid (missing required fields)".into())
                } else if count == 0 {
                    Some("No nodes found".into())
                } else {
                    None
                };

                SubStatus {
                    url: url.clone(),
                    success: count > 0,
                    node_count: count,
                    disabled_count: 0,
                    state: if count > 0 {
                        SubscriptionState::Ready
                    } else {
                        SubscriptionState::Failed
                    },
                    error: error_info,
                }
            }
            Err(e) => SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                disabled_count: 0,
                state: SubscriptionState::Failed,
                error: Some(e),
            },
        };
        let mut status_map = state.sub_status.lock().await;
        // Keep the generation check and status publication in the same
        // critical section so stale startup work cannot overwrite a newer
        // foreground result after waiting for this lock.
        if !refresh_generation_is_current(state, expected_generation) {
            break;
        }
        status_map.insert(url, status);
    }

    final_nodes
}

fn filter_informational_fetched_nodes(nodes: Vec<FetchedNode>) -> Vec<FetchedNode> {
    nodes
        .into_iter()
        .filter(|node| !is_informational_subscription_node(&node.name, &node.outbound))
        .collect()
}

/// Build a candidate without mutating the active runtime file or diagnostics.
async fn build_prepared(
    config: &Config,
    state: &Arc<AppState>,
    nodes: Vec<FetchedNode>,
) -> AppResult<GenConfigOutcome> {
    // Fetch already filters these entries; repeat at the build boundary so a
    // snapshot written by an older Miao version cannot bring them back.
    let nodes = filter_informational_fetched_nodes(nodes);
    // has_sub_nodes 是「订阅是否产出可用节点」的健康度语义（用户禁用过滤前）：
    // 下游用它区分「订阅获取失败」与「有节点」，禁用后为空是用户意图（全禁用），
    // 不能误报成订阅失败（ALL_SUBS_FAILED / KeptRunningOnTotalFailure）
    let has_sub_nodes = !nodes.is_empty();
    let nodes = filter_disabled_nodes(nodes, &config.disabled_nodes);
    let (my_outbounds, my_names) = collect_manual_outbounds(config);

    let manual_multipliers: Vec<_> = my_names.iter().map(|name| node_multiplier(name)).collect();
    let subscription_multipliers: Vec<_> = nodes
        .iter()
        .map(|node| node_multiplier(&node.name))
        .collect();
    let mut available_multipliers = BTreeSet::new();
    available_multipliers.extend(manual_multipliers.iter().flatten().copied());
    available_multipliers.extend(subscription_multipliers.iter().flatten().copied());

    let reserved_rule_tags = custom_rule_outbound_tags(&config.custom_rules);
    let (final_node_names, final_outbounds, node_bindings) =
        assign_subscription_tags(state, &my_names, &reserved_rule_tags, nodes).await;

    let (sing_box_config, skipped_rules, node_select) = build_sing_box_config_with_multipliers(
        config,
        my_names,
        my_outbounds,
        final_node_names,
        final_outbounds,
        manual_multipliers
            .into_iter()
            .chain(subscription_multipliers)
            .collect(),
    )?;

    Ok(GenConfigOutcome {
        bytes: serde_json::to_vec(&sing_box_config)?,
        has_sub_nodes,
        node_select,
        skipped_rules,
        available_multipliers: available_multipliers.into_iter().collect(),
        fresh_sub_nodes: None,
        node_bindings,
    })
}

/// 按易变层禁用集过滤订阅节点：按「订阅 + 节点名」匹配，订阅内同名节点连坐禁用。
/// 只在构建入口调用——快照保存的是完整拉取结果，禁用不影响快照内容。
pub(super) fn filter_disabled_nodes(
    nodes: Vec<FetchedNode>,
    disabled: &[DisabledNode],
) -> Vec<FetchedNode> {
    if disabled.is_empty() {
        return nodes;
    }
    let mut disabled_by_source: std::collections::HashMap<String, std::collections::HashSet<&str>> =
        std::collections::HashMap::new();
    for entry in disabled {
        disabled_by_source
            .entry(subscription_source_id(&entry.sub))
            .or_default()
            .insert(entry.name.as_str());
    }
    nodes
        .into_iter()
        .filter(|node| {
            disabled_by_source
                .get(&node.source_id)
                .is_none_or(|names| !names.contains(node.name.as_str()))
        })
        .collect()
}

fn custom_rule_outbound_tags(custom_rules: &[String]) -> Vec<String> {
    let mut tags = Vec::new();
    for raw in custom_rules {
        let Some(tag) = serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|rule| rule.get("outbound")?.as_str().map(str::to_string))
        else {
            continue;
        };
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// 在配置事务真正提交后发布候选配置携带的诊断元数据。
pub async fn publish_generation_diagnostics(state: &AppState, outcome: &GenConfigOutcome) {
    *state.skipped_rules.lock().await = outcome.skipped_rules.clone();
    *state.available_multipliers.write().await = outcome.available_multipliers.clone();
}

/// 缓存启动后从本地 canonical 节点材料恢复倍率选项。运行时 outbound tag
/// 可能由稳定绑定保留历史名称，不能用于重新判断倍率；订阅使用快照中的当前
/// 显示名，手动节点使用 config 中的当前 tag。快照缺失/不匹配时宁可只发布
/// 手动节点倍率，等待后台刷新补齐，也不发布错误选项。
pub async fn publish_runtime_multiplier_options(config: &Config, state: &AppState) {
    let mut multipliers = BTreeSet::new();
    let (_, manual_names) = collect_manual_outbounds(config);
    multipliers.extend(manual_names.iter().filter_map(|name| node_multiplier(name)));

    if let Some(snapshot) = read_sub_nodes_snapshot(state).await {
        if snapshot.matches_subs(&config.subs) {
            let nodes = filter_disabled_nodes(
                filter_informational_fetched_nodes(snapshot.into_fetched_nodes()),
                &config.disabled_nodes,
            );
            multipliers.extend(nodes.iter().filter_map(|node| node_multiplier(&node.name)));
        }
    }
    *state.available_multipliers.write().await = multipliers.into_iter().collect();
}

pub fn collect_manual_outbounds(config: &Config) -> (Vec<serde_json::Value>, Vec<String>) {
    use crate::services::node_parser::parse_node_json;

    let mut my_outbounds = vec![];
    let mut my_names = vec![];

    for (idx, node_str) in config.nodes.iter().enumerate() {
        // 验证节点并获取解析后的 Value
        match parse_node_json(node_str) {
            Ok((info, outbound)) => {
                my_names.push(info.tag);
                my_outbounds.push(outbound);
            }
            Err(e) => {
                warn!("[collect_manual_outbounds] Skipping node #{}: {}", idx, e);
            }
        }
    }

    (my_outbounds, my_names)
}

/// 从运行时 sing-box 配置中提取用户节点 tag(排除内置 outbound)
pub(super) fn runtime_config_node_tags(config_json: &serde_json::Value) -> Vec<String> {
    config_json["outbounds"]
        .as_array()
        .map(|outbounds| {
            outbounds
                .iter()
                .filter_map(|outbound| outbound["tag"].as_str())
                .filter(|tag| *tag != "proxy" && *tag != "direct")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 自定义规则可选的节点目标:手动节点(配置直读)+ 订阅节点(最近一次生成的运行时配置)。
/// 仅供规则校验给出友好报错,最终正确性由 sing-box check 保证。
pub async fn known_rule_targets(config: &Config, state: &AppState) -> Vec<String> {
    use crate::services::node_parser::parse_node_json;

    let mut tags: Vec<String> = Vec::new();
    for node_str in &config.nodes {
        if let Ok((info, _)) = parse_node_json(node_str) {
            if !tags.contains(&info.tag) {
                tags.push(info.tag);
            }
        }
    }

    // Existing rules reserve their target even while the node is absent. This
    // is the fail-closed fallback when node-bindings.json is missing or invalid:
    // a future same-name node must not silently inherit a dormant rule.
    for tag in custom_rule_outbound_tags(&config.custom_rules) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    let runtime_config_path = &state.runtime_paths.active_config;
    if let Ok(content) = tokio::fs::read_to_string(&runtime_config_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            for tag in runtime_config_node_tags(&json) {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
    }

    for tag in reserved_node_tags(state).await {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    tags
}

#[cfg(test)]
mod tests {
    use super::{
        filter_disabled_nodes, filter_informational_fetched_nodes, known_rule_targets,
        subscription_source_id, FetchedNode, SubFetchRetry, STARTUP_FETCH_BUDGET,
        STARTUP_RETRY_SCHEDULE,
    };
    use crate::models::{Config, DisabledNode};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::Duration;

    fn fetched_node(sub_url: &str, name: &str) -> FetchedNode {
        FetchedNode {
            source_id: subscription_source_id(sub_url),
            name: name.to_string(),
            outbound: serde_json::json!({"type": "trojan", "server": "example.com"}),
        }
    }

    fn disabled_node(sub: &str, name: &str) -> DisabledNode {
        DisabledNode {
            sub: sub.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn informational_filter_sanitizes_nodes_loaded_from_old_snapshots() {
        let mut info = fetched_node("https://a", "流量信息");
        info.outbound["server"] = serde_json::json!("127.0.0.1");
        let expiry = fetched_node("https://a", "套餐到期：2026-09-17");
        let normal = fetched_node("https://a", "香港 01");

        let kept = filter_informational_fetched_nodes(vec![info, expiry, normal.clone()]);

        assert_eq!(kept, vec![normal]);
    }

    #[test]
    fn filter_disabled_nodes_keeps_everything_when_nothing_disabled() {
        let nodes = vec![fetched_node("https://a", "node-1")];
        let kept = filter_disabled_nodes(nodes.clone(), &[]);
        assert_eq!(kept, nodes);
    }

    #[test]
    fn filter_disabled_nodes_matches_by_subscription_and_name() {
        let nodes = vec![
            fetched_node("https://a", "node-1"),
            fetched_node("https://a", "node-2"),
            fetched_node("https://b", "node-1"),
        ];
        let kept = filter_disabled_nodes(nodes, &[disabled_node("https://a", "node-1")]);
        let names: Vec<_> = kept.iter().map(|n| n.name.as_str()).collect();
        // 只禁用 a 订阅的 node-1；b 订阅的同名节点保留
        assert_eq!(names, ["node-2", "node-1"]);
        assert_eq!(kept[1].source_id, subscription_source_id("https://b"));
    }

    #[test]
    fn filter_disabled_nodes_disables_same_name_duplicates_together() {
        // 订阅内同名重复节点连坐禁用
        let nodes = vec![
            fetched_node("https://a", "dup"),
            fetched_node("https://a", "dup"),
            fetched_node("https://a", "solo"),
        ];
        let kept = filter_disabled_nodes(nodes, &[disabled_node("https://a", "dup")]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "solo");
    }

    #[test]
    fn filter_disabled_nodes_can_empty_the_pool() {
        let nodes = vec![fetched_node("https://a", "node-1")];
        let kept = filter_disabled_nodes(nodes, &[disabled_node("https://a", "node-1")]);
        assert!(kept.is_empty());
    }

    #[tokio::test]
    async fn max_multiplier_only_limits_automatic_candidates_and_keeps_all_outbounds() {
        use crate::models::{NodeMultiplier, NodeSelect, Region};

        let config = Config {
            nodes: vec![
                r#"{"type":"trojan","tag":"未标倍率手动节点","server":"manual.example.com","server_port":443,"password":"password123"}"#.to_string(),
                r#"{"type":"trojan","tag":"日本手动节点 6x","server":"expensive.example.com","server_port":443,"password":"password123"}"#.to_string(),
                r#"{"type":"trojan","tag":"日本无效倍率 1.2345x","server":"invalid.example.com","server_port":443,"password":"password123"}"#.to_string(),
            ],
            node_select: NodeSelect::Fastest(Region::Jp),
            max_multiplier: NodeMultiplier::parse("2.5"),
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());
        let fetched = vec![
            fetched_node("https://a", "日本[1.3x]-普通"),
            fetched_node("https://a", "日本[18x]-专线"),
        ];

        let outcome = super::build_prepared(&config, &state, fetched)
            .await
            .unwrap();
        let generated: serde_json::Value = serde_json::from_slice(&outcome.bytes).unwrap();
        let outbounds = generated["outbounds"].as_array().unwrap();
        let tags: Vec<&str> = outbounds
            .iter()
            .filter_map(|outbound| outbound["tag"].as_str())
            .collect();

        assert!(tags.contains(&"未标倍率手动节点"));
        assert!(tags.contains(&"日本手动节点 6x"));
        assert!(tags.contains(&"日本无效倍率 1.2345x"));
        assert!(tags.contains(&"日本[1.3x]-普通"));
        assert!(tags.contains(&"日本[18x]-专线"));
        assert_eq!(outbounds[0]["type"], "urltest");
        assert_eq!(
            outbounds[0]["outbounds"],
            serde_json::json!(["日本[1.3x]-普通"])
        );
        assert_eq!(
            outcome
                .available_multipliers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["1", "1.3", "6", "18"]
        );
    }

    #[tokio::test]
    async fn max_multiplier_uses_current_subscription_name_when_stable_tag_is_old() {
        use crate::models::{NodeMultiplier, NodeSelect, Region};
        use crate::services::config::bindings::save_node_bindings;

        let config = Config {
            node_select: NodeSelect::Fastest(Region::Jp),
            max_multiplier: NodeMultiplier::parse("2.5"),
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());
        let mut old_name = fetched_node("https://a", "日本[1.3x]-将改名");
        old_name.outbound["server"] = serde_json::json!("a.example.com");
        let mut low = fetched_node("https://a", "日本[1.3x]-保留");
        low.outbound["server"] = serde_json::json!("b.example.com");
        let first = super::build_prepared(&config, &state, vec![old_name.clone(), low.clone()])
            .await
            .unwrap();
        save_node_bindings(&state, &first.node_bindings)
            .await
            .unwrap();

        // 同一节点只改显示名/倍率，稳定 tag 仍保留旧的 1.3x 文本。
        old_name.name = "日本[18x]-已改名".to_string();
        let second = super::build_prepared(&config, &state, vec![old_name, low])
            .await
            .unwrap();
        let generated: serde_json::Value = serde_json::from_slice(&second.bytes).unwrap();

        assert_eq!(
            generated["outbounds"][0]["outbounds"],
            serde_json::json!(["日本[1.3x]-保留"]),
            "18x 节点即使稳定 tag 仍写 1.3x，也不能进入自动候选"
        );
        assert!(generated["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|outbound| outbound["tag"] == "日本[1.3x]-将改名"));
    }

    #[tokio::test]
    async fn cached_multiplier_options_use_snapshot_names_instead_of_stable_tags() {
        use crate::services::config::{save_sub_nodes_snapshot, SubNodesSnapshot};

        let sub = "https://a".to_string();
        let config = Config {
            subs: vec![sub.clone()],
            nodes: vec![
                r#"{"type":"trojan","tag":"手动节点 6x","server":"manual.example.com","server_port":443,"password":"password123"}"#.to_string(),
            ],
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());
        let snapshot = SubNodesSnapshot::from_fetched_nodes(
            vec![sub],
            vec![fetched_node("https://a", "日本[18x]-当前名称")],
        );
        save_sub_nodes_snapshot(&state, &snapshot).await.unwrap();
        tokio::fs::create_dir_all(&state.runtime_paths.runtime_dir)
            .await
            .unwrap();
        tokio::fs::write(
            &state.runtime_paths.active_config,
            serde_json::to_vec(&serde_json::json!({
                "outbounds": [
                    { "type": "selector", "tag": "proxy", "outbounds": ["日本[1.3x]-旧稳定标签", "手动节点 6x"] },
                    { "type": "direct", "tag": "direct" },
                    { "type": "trojan", "tag": "日本[1.3x]-旧稳定标签" },
                    { "type": "trojan", "tag": "手动节点 6x" }
                ]
            }))
            .unwrap(),
        )
        .await
        .unwrap();

        super::publish_runtime_multiplier_options(&config, &state).await;

        assert_eq!(
            state
                .available_multipliers
                .read()
                .await
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["6", "18"]
        );
        let _ = tokio::fs::remove_dir_all(&state.runtime_paths.runtime_dir).await;
    }

    #[tokio::test]
    async fn max_multiplier_is_inactive_in_manual_mode() {
        use crate::models::NodeMultiplier;

        let config = Config {
            nodes: vec![
                r#"{"type":"trojan","tag":"普通节点","server":"normal.example.com","server_port":443,"password":"password123"}"#.to_string(),
                r#"{"type":"trojan","tag":"高倍率 18x","server":"expensive.example.com","server_port":443,"password":"password123"}"#.to_string(),
            ],
            max_multiplier: NodeMultiplier::parse("2.5"),
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());

        let outcome = super::build_prepared(&config, &state, Vec::new())
            .await
            .unwrap();
        let generated: serde_json::Value = serde_json::from_slice(&outcome.bytes).unwrap();

        assert_eq!(generated["outbounds"][0]["type"], "selector");
        assert_eq!(
            generated["outbounds"][0]["outbounds"],
            serde_json::json!(["普通节点", "高倍率 18x"])
        );
    }

    #[tokio::test]
    async fn has_sub_nodes_reflects_fetch_health_not_the_disable_filter() {
        // 回归：全禁用是用户意图，has_sub_nodes 必须保持过滤前的「订阅产出过节点」
        // 语义——否则会被误报成「所有订阅获取失败」（ALL_SUBS_FAILED）并被
        // Startup 刷新路径误判为 KeptRunningOnTotalFailure
        // 手动节点兑底保证空池保护（NoUsableNodes）不拦截，专注验证 has_sub_nodes 语义
        let config = Config {
            subs: vec!["https://a".to_string()],
            nodes: vec![
                r#"{"type":"trojan","tag":"manual-1","server":"m.example.com","server_port":443,"password":"x"}"#
                    .to_string(),
            ],
            disabled_nodes: vec![disabled_node("https://a", "node-1")],
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());

        let outcome =
            super::build_prepared(&config, &state, vec![fetched_node("https://a", "node-1")])
                .await
                .unwrap();

        assert!(outcome.has_sub_nodes);
        // 且被禁节点确实不在生成结果里
        let text = String::from_utf8(outcome.bytes).unwrap();
        assert!(!text.contains("node-1"));
    }

    #[tokio::test]
    async fn dormant_custom_rule_target_remains_reserved_without_bindings() {
        let config = Config {
            custom_rules: vec![
                r#"{"domain_suffix":"example.com","action":"route","outbound":"gone-node"}"#
                    .to_string(),
            ],
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());

        let targets = known_rule_targets(&config, &state).await;

        assert!(targets.contains(&"gone-node".to_string()));
    }

    #[test]
    fn startup_retry_schedule_fits_inside_the_absolute_budget() {
        assert_eq!(
            STARTUP_RETRY_SCHEDULE,
            &[
                Duration::from_secs(2),
                Duration::from_secs(5),
                Duration::from_secs(10)
            ]
        );
        assert!(STARTUP_RETRY_SCHEDULE.iter().sum::<Duration>() < STARTUP_FETCH_BUDGET);
    }

    /// 本地「计数拒答」订阅服务器：每接受一个 TCP 连接就计数并立即挂断，
    /// 让 fetch 以连接错误快速失败。用拉取次数而非耗时断言重试行为，
    /// 跨平台无时间敏感（Windows CI 上 loopback 拒连耗时不稳定）
    async fn counting_sub_server(attempts: Arc<AtomicUsize>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind counting server");
        let port = listener.local_addr().expect("local addr").port();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                attempts.fetch_add(1, Ordering::Relaxed);
                drop(socket);
            }
        });
        format!("http://127.0.0.1:{port}/sub")
    }

    #[tokio::test]
    async fn startup_retries_total_failure_until_budget_exhausted() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let url = counting_sub_server(attempts.clone()).await;
        let config = Config {
            subs: vec![url],
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());

        // 无手动节点时 gen_config 返回 NoUsableNodes，不写 config.json
        let err = match tokio::time::timeout(
            Duration::from_secs(5),
            super::gen_config(&config, &state, SubFetchRetry::Startup),
        )
        .await
        {
            Ok(Ok(_)) => panic!("failing subscription must fail"),
            Ok(Err(err)) => err,
            Err(_) => panic!("gen_config exceeded 5s with the shortened test schedule"),
        };

        assert!(err.is_no_usable_nodes());
        // 初次 + 测试调度（50ms/100ms）的两段退避 = 恰好 3 次拉取
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn no_retry_returns_after_a_single_attempt() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let url = counting_sub_server(attempts.clone()).await;
        let config = Config {
            subs: vec![url],
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());

        let err = match tokio::time::timeout(
            Duration::from_secs(5),
            super::gen_config(&config, &state, SubFetchRetry::None),
        )
        .await
        {
            Ok(Ok(_)) => panic!("failing subscription must fail"),
            Ok(Err(err)) => err,
            Err(_) => panic!("gen_config exceeded 5s without any retry budget"),
        };

        assert!(err.is_no_usable_nodes());
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }
}
