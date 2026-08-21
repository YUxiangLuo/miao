use futures::{stream, StreamExt};
use sha2::{Digest, Sha256};
use std::sync::{atomic::Ordering, Arc};
use tokio::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::error::AppResult;
use crate::models::{Config, NodeSelect, SubStatus, SubscriptionState};
use crate::services::subscription::fetch_sub;
use crate::state::{AppState, SkippedRule};

use super::bindings::{assign_subscription_tags, reserved_node_tags, NodeTagBindings};
use super::builder::build_sing_box_config;
use super::persist::{read_sub_nodes_snapshot, save_sub_nodes_snapshot, SubNodesSnapshot};

#[derive(Clone)]
pub struct GenConfigOutcome {
    pub bytes: Vec<u8>,
    pub has_sub_nodes: bool,
    pub node_select: NodeSelect,
    pub skipped_rules: Vec<SkippedRule>,
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

fn subscription_source_id(url: &str) -> String {
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

/// Build a candidate without mutating the active runtime file or diagnostics.
async fn build_prepared(
    config: &Config,
    state: &Arc<AppState>,
    nodes: Vec<FetchedNode>,
) -> AppResult<GenConfigOutcome> {
    let (my_outbounds, my_names) = collect_manual_outbounds(config);
    let has_sub_nodes = !nodes.is_empty();
    let reserved_rule_tags = custom_rule_outbound_tags(&config.custom_rules);
    let (final_node_names, final_outbounds, node_bindings) =
        assign_subscription_tags(state, &my_names, &reserved_rule_tags, nodes).await;

    let (sing_box_config, skipped_rules, node_select) = build_sing_box_config(
        config,
        my_names,
        my_outbounds,
        final_node_names,
        final_outbounds,
    )?;

    Ok(GenConfigOutcome {
        bytes: serde_json::to_vec(&sing_box_config)?,
        has_sub_nodes,
        node_select,
        skipped_rules,
        fresh_sub_nodes: None,
        node_bindings,
    })
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

pub(super) fn collect_manual_outbounds(config: &Config) -> (Vec<serde_json::Value>, Vec<String>) {
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
    use super::{known_rule_targets, SubFetchRetry, STARTUP_FETCH_BUDGET, STARTUP_RETRY_SCHEDULE};
    use crate::models::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::Duration;

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
