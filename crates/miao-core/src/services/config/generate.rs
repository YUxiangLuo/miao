use futures::{stream, StreamExt};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::sync::{atomic::Ordering, Arc};
use tokio::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::error::AppResult;
use crate::models::{
    node_multiplier, Config, DisabledNode, NodeMultiplier, NodeSelect, SubStatus,
    SubscriptionFailureKind, SubscriptionFetchReport, SubscriptionState,
};
use crate::services::subscription::{fetch_sub, is_informational_subscription_node, FetchResult};
use crate::state::{AppState, BlockedAccessPort, SkippedRule};

use super::bindings::{assign_subscription_tags, reserved_node_tags, NodeTagBindings};
use super::builder::{
    blocked_access_ports_for_runtime, build_sing_box_config_with_multipliers, NodeMetadata,
    ParsedCustomRules,
};
use super::persist::{read_sub_nodes_snapshot, save_sub_nodes_snapshot, SubNodesSnapshot};

#[derive(Clone)]
pub struct GenConfigOutcome {
    pub bytes: Vec<u8>,
    /// Availability before user filters, including retained cached nodes.
    pub has_sub_nodes: bool,
    /// None for local rebuilds; never infer network health from node count.
    pub subscription_fetch: Option<SubscriptionFetchReport>,
    pub node_select: NodeSelect,
    pub skipped_rules: Vec<SkippedRule>,
    pub blocked_access_ports: Vec<BlockedAccessPort>,
    /// 完整可用节点池中识别出的倍率，供面板动态生成自动候选上限选项。
    pub available_multipliers: Vec<NodeMultiplier>,
    /// 本轮接受的节点集（含失败来源保留的节点，允许成功空列表）；本地重建为 None。
    /// 校验通过/启动成功后由 record_fresh_snapshot 落盘，供本地语义变更零网络重建。
    pub fresh_sub_nodes: Option<Vec<FetchedNode>>,
    pub node_bindings: NodeTagBindings,
}

impl GenConfigOutcome {
    pub fn subscription_fetch_failed(&self) -> bool {
        self.subscription_fetch
            .is_some_and(SubscriptionFetchReport::total_failure)
    }

    pub fn accepted_subscription_response(&self) -> bool {
        self.subscription_fetch
            .is_some_and(SubscriptionFetchReport::accepted_response)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FetchedNode {
    pub source_id: String,
    pub name: String,
    pub outbound: serde_json::Value,
}

/// A fetch result keeps availability separate from network health. Cached
/// nodes may serve failed sources, but must never end startup's recovery loop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FetchedSubscriptions {
    pub nodes: Vec<FetchedNode>,
    pub report: SubscriptionFetchReport,
}

pub async fn gen_config_from_fetch(
    config: &Config,
    state: &Arc<AppState>,
    fetched: FetchedSubscriptions,
) -> AppResult<GenConfigOutcome> {
    let mut outcome = build_prepared(config, state, fetched.nodes.clone()).await?;
    outcome.subscription_fetch = Some(fetched.report);
    outcome.node_bindings.subscription_snapshot_sha256 = Some(
        SubNodesSnapshot::from_fetched_nodes(config.subs.clone(), fetched.nodes.clone())
            .fingerprint()?,
    );
    // Includes successful empty sources: an old snapshot must not resurrect
    // nodes intentionally removed by a provider on the next local edit.
    outcome.fresh_sub_nodes = Some(fetched.nodes);
    Ok(outcome)
}

pub fn subscription_source_id(url: &str) -> String {
    hex::encode(Sha256::digest(url.as_bytes()))
}

/// Removing a source is a local edit when the accepted snapshot can identify
/// every node's owner. Never borrow nodes from a removed or legacy source.
pub(super) async fn retained_subscription_nodes(
    before: &Config,
    candidate: &Config,
    state: &Arc<AppState>,
) -> Option<FetchedSubscriptions> {
    if candidate.subs.is_empty() {
        return Some(FetchedSubscriptions::default());
    }
    let snapshot = read_sub_nodes_snapshot(state).await?;
    if !snapshot.has_complete_source_ownership(&before.subs) {
        return None;
    }
    let retained_sources: BTreeSet<_> = candidate
        .subs
        .iter()
        .map(|url| subscription_source_id(url))
        .collect();
    let nodes: Vec<_> = filter_informational_fetched_nodes(snapshot.to_fetched_nodes())
        .into_iter()
        .filter(|node| retained_sources.contains(&node.source_id))
        .collect();
    // Disabled nodes and broken detour chains remain canonical, but cannot
    // keep the service available. Use the same build boundary as activation;
    // otherwise a broken manual fallback can prevent fetching a usable source.
    if build_prepared(candidate, state, nodes.clone())
        .await
        .is_err()
    {
        return None;
    }
    Some(FetchedSubscriptions {
        report: SubscriptionFetchReport {
            cached_nodes: nodes.len(),
            ..Default::default()
        },
        nodes,
    })
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

/// Test convenience for fetch + generation. Production explicitly separates
/// fetching from generation so installation cannot perform locked network I/O.
#[cfg(test)]
pub async fn gen_config(
    config: &Config,
    state: &Arc<AppState>,
    retry: SubFetchRetry,
) -> AppResult<GenConfigOutcome> {
    let nodes = fetch_sub_nodes(config, state, retry).await;
    gen_config_from_fetch(config, state, nodes).await
}

/// 只拉取订阅节点集（不写盘）：订阅全失败时按 retry 预算退避重试。
/// 供「先拉取、后持锁落地」的调用方（启动后台刷新）把网络等待移出配置锁。
#[cfg(test)]
async fn fetch_sub_nodes(
    config: &Config,
    state: &Arc<AppState>,
    retry: SubFetchRetry,
) -> FetchedSubscriptions {
    let generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    fetch_sub_nodes_if_current(config, state, retry, generation).await
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
) -> FetchedSubscriptions {
    let cancelled = state.sub_refresh_cancel.notified();
    tokio::pin!(cancelled);
    cancelled.as_mut().enable();
    if !refresh_generation_is_current(state, Some(expected_generation)) {
        return FetchedSubscriptions::default();
    }
    let activity = state.subscription_refresh.begin(expected_generation);
    tokio::select! {
        result = fetch_sub_nodes_inner(config, state, retry, Some(expected_generation)) => {
            if refresh_generation_is_current(state, Some(expected_generation)) {
                activity.finish(result.report);
            }
            result
        },
        _ = cancelled => FetchedSubscriptions::default(),
    }
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
) -> FetchedSubscriptions {
    let schedule = retry.schedule();
    let deadline =
        matches!(retry, SubFetchRetry::Startup).then(|| Instant::now() + startup_fetch_budget());
    let mut attempt = 0usize;
    let mut last_failure = FetchedSubscriptions {
        report: SubscriptionFetchReport {
            failed_sources: config.subs.len(),
            ..Default::default()
        },
        ..Default::default()
    };
    loop {
        if !refresh_generation_is_current(state, expected_generation) {
            return FetchedSubscriptions::default();
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            warn!("Startup subscription refresh budget exhausted");
            return last_failure;
        }

        let nodes = fetch_all_subs(config, state, deadline, expected_generation).await;
        if !refresh_generation_is_current(state, expected_generation) {
            return FetchedSubscriptions::default();
        }
        // Successful empty lists are authoritative, not transient network failures.
        // Cached availability and user selection filters do not affect retries.
        if !nodes.report.total_failure() {
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
        last_failure = nodes;
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
    if let Some(nodes) = &fresh {
        outcome.node_bindings.subscription_snapshot_sha256 = Some(
            SubNodesSnapshot::from_fetched_nodes(config.subs.clone(), nodes.clone())
                .fingerprint()?,
        );
    }
    outcome.fresh_sub_nodes = fresh;
    Ok(outcome)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OfflineManualFallbackDecision {
    /// Active runtime has no subscription nodes (or neither active runtime nor
    /// cache exists), so rebuilding from manual nodes drops nothing active.
    RebuildFromManualNodes,
    /// Active runtime contains subscription nodes, or active runtime is unreadable
    /// while a config cache exists: refuse to drop subscription nodes silently.
    RejectMissingSnapshot,
}

fn decide_offline_manual_fallback(
    manual_names: &[String],
    active_runtime: Option<&serde_json::Value>,
    has_cache: bool,
) -> OfflineManualFallbackDecision {
    let has_subscription_runtime = active_runtime.is_some_and(|json| {
        runtime_config_node_tags(json)
            .iter()
            .any(|tag| !manual_names.contains(tag))
    });
    if has_subscription_runtime || (active_runtime.is_none() && has_cache) {
        OfflineManualFallbackDecision::RejectMissingSnapshot
    } else {
        OfflineManualFallbackDecision::RebuildFromManualNodes
    }
}

/// 用订阅节点集快照零网络重建配置；缺失时仅允许可证明完整的手动节点重建。
/// 本地语义变更（节点选择/路由模式/规则/手动节点）走这里：
/// 切换不是刷新，不该被订阅网络故障拖累。
pub async fn gen_config_from_snapshot(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<GenConfigOutcome> {
    if let Some(snapshot) = read_sub_nodes_snapshot(state).await {
        if snapshot.matches_subs(&config.subs) {
            info!("Rebuilding config from subscription node snapshot (no network)");
            return build_prepared(config, state, snapshot.to_fetched_nodes()).await;
        }
        warn!("Subscription snapshot does not match the requested sources");
    }
    if config.subs.is_empty() {
        return build_prepared(config, state, Vec::new()).await;
    }
    // Local edits run inside the config transaction. Missing subscription
    // material must not turn them into unbounded network operations. A runtime
    // containing only known manual nodes can still be edited offline.
    let old = state.config.read().await.clone();
    let (_, manual_names) = collect_manual_outbounds(&old);
    let active = tokio::fs::read(&state.runtime_paths.active_config)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    match decide_offline_manual_fallback(
        &manual_names,
        active.as_ref(),
        super::persist::has_config_cache(state),
    ) {
        OfflineManualFallbackDecision::RebuildFromManualNodes => {
            build_prepared(config, state, Vec::new()).await
        }
        OfflineManualFallbackDecision::RejectMissingSnapshot => {
            Err(crate::error::AppError::message(
                "订阅节点快照不可用，当前运行配置已保留；请先刷新订阅后重试",
            ))
        }
    }
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

type RawSubFetchResult = Result<FetchResult, (SubscriptionFailureKind, String)>;

async fn load_cached_nodes_by_source(state: &AppState) -> HashMap<String, Vec<FetchedNode>> {
    let mut cached: HashMap<String, Vec<FetchedNode>> = HashMap::new();
    if let Some(snapshot) = read_sub_nodes_snapshot(state).await {
        for node in snapshot.to_fetched_nodes() {
            cached.entry(node.source_id.clone()).or_default().push(node);
        }
    }
    cached
}

async fn mark_subscriptions_refreshing(
    config: &Config,
    state: &AppState,
    expected_generation: Option<u64>,
) -> bool {
    let mut status_map = state.sub_status.lock().await;
    // Re-check after acquiring the status lock. A foreground refresh may
    // have advanced the generation while this task was waiting for it.
    if !refresh_generation_is_current(state, expected_generation) {
        return false;
    }
    status_map.retain(|url, _| config.subs.contains(url));
    for url in &config.subs {
        let status = status_map.entry(url.clone()).or_insert_with(|| SubStatus {
            url: url.clone(),
            success: false,
            node_count: 0,
            disabled_count: 0,
            state: SubscriptionState::Pending,
            failure_kind: None,
            error: None,
        });
        status.state = SubscriptionState::Refreshing;
    }
    drop(status_map);
    state.data_revision.fetch_add(1, Ordering::Relaxed);
    true
}

async fn fetch_single_subscription(
    sub: String,
    client: reqwest::Client,
    deadline: Option<Instant>,
) -> (String, RawSubFetchResult) {
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

            (sub, Ok(fetch_result))
        }
        Ok(Err(e)) => {
            error!(url = %sub, error = %e, "Failed to fetch subscription");
            (sub, Err((e.kind, e.to_string())))
        }
        Err(_) => {
            error!(url = %sub, timeout_ms = request_budget.as_millis(), "Subscription fetch timed out");
            let message = if deadline.is_some() {
                "Startup refresh budget exhausted"
            } else {
                "Request timeout"
            };
            (
                sub,
                Err((SubscriptionFailureKind::Timeout, message.to_string())),
            )
        }
    }
}

/// A body whose non-informational entries all failed parsing is a failed source,
/// not an authoritative empty list. Keep its last good nodes just like HTTP errors.
fn promote_total_parse_failure(fetched: FetchResult) -> RawSubFetchResult {
    if fetched.node_names.is_empty() && fetched.total_count > fetched.filtered_info_count {
        Err((
            SubscriptionFailureKind::Parse,
            "All subscription nodes failed to parse; keeping previous nodes".to_string(),
        ))
    } else {
        Ok(fetched)
    }
}

fn accepted_fetch_diagnostic(
    valid_count: usize,
    total_count: usize,
    filtered_info_count: usize,
    parse_error_count: usize,
) -> Option<String> {
    if parse_error_count > 0 {
        Some(format!(
            "{parse_error_count} nodes skipped due to parse errors"
        ))
    } else if valid_count == 0 && filtered_info_count > 0 {
        Some(format!(
            "No proxy nodes found ({filtered_info_count} informational entries filtered)"
        ))
    } else if valid_count == 0 && total_count > 0 {
        Some("All nodes invalid (missing required fields)".into())
    } else if valid_count == 0 {
        Some("No nodes found".into())
    } else {
        None
    }
}

struct SourceFetchResolution {
    status: SubStatus,
    nodes: Vec<FetchedNode>,
    fresh_nodes: usize,
    cached_nodes: usize,
    succeeded: bool,
}

fn resolve_source_fetch(
    url: &str,
    raw_result: RawSubFetchResult,
    cached_by_source: &mut HashMap<String, Vec<FetchedNode>>,
) -> SourceFetchResolution {
    match raw_result.and_then(promote_total_parse_failure) {
        Ok(fetch_result) => {
            let count = fetch_result.node_names.len();
            let error_info = accepted_fetch_diagnostic(
                count,
                fetch_result.total_count,
                fetch_result.filtered_info_count,
                fetch_result.parse_errors.len(),
            );
            let source_id = subscription_source_id(url);
            let nodes = fetch_result
                .node_names
                .into_iter()
                .zip(fetch_result.outbounds)
                .map(|(name, outbound)| FetchedNode {
                    source_id: source_id.clone(),
                    name,
                    outbound,
                })
                .collect();

            SourceFetchResolution {
                status: SubStatus {
                    url: url.to_string(),
                    success: true,
                    node_count: count,
                    disabled_count: 0,
                    state: SubscriptionState::Ready,
                    failure_kind: None,
                    error: error_info,
                },
                nodes,
                fresh_nodes: count,
                cached_nodes: 0,
                succeeded: true,
            }
        }
        Err((kind, message)) => {
            let old_nodes = cached_by_source
                .remove(&subscription_source_id(url))
                .unwrap_or_default();
            let count = old_nodes.len();
            SourceFetchResolution {
                status: SubStatus {
                    url: url.to_string(),
                    success: false,
                    node_count: count,
                    disabled_count: 0,
                    state: SubscriptionState::Failed,
                    failure_kind: Some(kind),
                    error: Some(message),
                },
                nodes: old_nodes,
                fresh_nodes: 0,
                cached_nodes: count,
                succeeded: false,
            }
        }
    }
}

/// 并发拉取全部订阅并逐条更新 sub_status；返回合并后的节点名与 outbounds。
async fn fetch_all_subs(
    config: &Config,
    state: &Arc<AppState>,
    deadline: Option<Instant>,
    expected_generation: Option<u64>,
) -> FetchedSubscriptions {
    let mut fetched = FetchedSubscriptions::default();
    let mut cached = load_cached_nodes_by_source(state).await;
    if !mark_subscriptions_refreshing(config, state, expected_generation).await {
        return fetched;
    }

    let sub_futures = config
        .subs
        .iter()
        .cloned()
        .map(|sub| fetch_single_subscription(sub, state.http_client.clone(), deadline));

    // 使用 buffer_unordered 限制并发数，避免同时发起过多请求
    let mut results: Vec<_> = stream::iter(sub_futures)
        .buffer_unordered(MAX_CONCURRENT_SUBS)
        .collect()
        .await;

    // 按原始顺序排序结果
    results.sort_by_key(|(url, _)| {
        config
            .subs
            .iter()
            .position(|s| s == url)
            .unwrap_or(usize::MAX)
    });

    for (url, result) in results {
        if !refresh_generation_is_current(state, expected_generation) {
            break;
        }
        let resolution = resolve_source_fetch(&url, result, &mut cached);
        if resolution.succeeded {
            fetched.report.successful_sources += 1;
            fetched.report.fresh_nodes += resolution.fresh_nodes;
        } else {
            fetched.report.failed_sources += 1;
            fetched.report.cached_nodes += resolution.cached_nodes;
        }
        fetched.nodes.extend(resolution.nodes);

        let mut status_map = state.sub_status.lock().await;
        // Keep the generation check and status publication in the same
        // critical section so stale startup work cannot overwrite a newer
        // foreground result after waiting for this lock.
        if !refresh_generation_is_current(state, expected_generation) {
            break;
        }
        status_map.insert(url, resolution.status);
        state.data_revision.fetch_add(1, Ordering::Relaxed);
    }

    fetched
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
    // Availability only: cached nodes count too. Disabled/region/multiplier
    // filters are user intent and must not change subscription fetch health.
    let has_sub_nodes = !nodes.is_empty();
    let nodes = filter_disabled_nodes(nodes, &config.disabled_nodes);
    let (my_outbounds, my_names) = collect_manual_outbounds(config);

    let metadata: Vec<_> = my_names
        .iter()
        .chain(nodes.iter().map(|node| &node.name))
        .map(|name| NodeMetadata {
            display_name: name.clone(),
            multiplier: node_multiplier(name),
        })
        .collect();
    let available_multipliers: BTreeSet<_> =
        metadata.iter().filter_map(|meta| meta.multiplier).collect();
    let parsed_rules = ParsedCustomRules::parse(&config.custom_rules);
    let mut reserved_rule_tags = parsed_rules.outbound_tags();
    for ap in &config.access_ports {
        if ap.outbound != "proxy"
            && ap.outbound != "direct"
            && ap.outbound != "reject"
            && !reserved_rule_tags.contains(&ap.outbound)
        {
            reserved_rule_tags.push(ap.outbound.clone());
        }
    }
    let (final_node_names, final_outbounds, node_bindings) =
        assign_subscription_tags(state, &my_names, &reserved_rule_tags, nodes).await;

    let (sing_box_config, skipped_rules, node_select) = build_sing_box_config_with_multipliers(
        config,
        my_names,
        my_outbounds,
        final_node_names,
        final_outbounds,
        metadata,
        parsed_rules,
    )?;
    let blocked_access_ports =
        blocked_access_ports_for_runtime(&config.access_ports, &sing_box_config);

    Ok(GenConfigOutcome {
        bytes: serde_json::to_vec(&sing_box_config)?,
        has_sub_nodes,
        subscription_fetch: None,
        node_select,
        skipped_rules,
        blocked_access_ports,
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

/// 在配置事务真正提交后发布候选配置携带的诊断元数据。
pub async fn publish_generation_diagnostics(state: &AppState, outcome: &GenConfigOutcome) {
    *state.skipped_rules.lock().await = outcome.skipped_rules.clone();
    *state.blocked_access_ports.lock().await = outcome.blocked_access_ports.clone();
    *state.available_multipliers.write().await = outcome.available_multipliers.clone();
    state.data_revision.fetch_add(1, Ordering::Relaxed);
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
                filter_informational_fetched_nodes(snapshot.to_fetched_nodes()),
                &config.disabled_nodes,
            );
            multipliers.extend(nodes.iter().filter_map(|node| node_multiplier(&node.name)));
        }
    }
    *state.available_multipliers.write().await = multipliers.into_iter().collect();
}

/// Cached runtime bytes already enforce blocking; restore the matching panel
/// diagnostics as well, including startup without a subscription refresh.
pub async fn publish_cached_rule_diagnostics(config: &Config, state: &AppState) {
    let Ok(bytes) = tokio::fs::read(&state.runtime_paths.active_config).await else {
        return;
    };
    let Ok(runtime) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return;
    };
    let available = runtime["outbounds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|outbound| outbound["tag"].as_str().map(str::to_string))
        .collect();
    *state.skipped_rules.lock().await =
        ParsedCustomRules::parse(&config.custom_rules).missing_outbound_diagnostics(&available);
    *state.blocked_access_ports.lock().await =
        blocked_access_ports_for_runtime(&config.access_ports, &runtime);
    state.data_revision.fetch_add(1, Ordering::Relaxed);
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

fn push_unique_tag(tags: &mut Vec<String>, tag: String) {
    if !tags.contains(&tag) {
        tags.push(tag);
    }
}

/// 自定义规则可选的节点目标:手动节点(配置直读)+ 订阅节点(最近一次生成的运行时配置)。
/// 仅供规则校验给出友好报错,最终正确性由 sing-box check 保证。
pub async fn known_rule_targets(config: &Config, state: &AppState) -> Vec<String> {
    use crate::services::node_parser::parse_node_json;

    let mut tags: Vec<String> = Vec::new();
    for node_str in &config.nodes {
        if let Ok((info, _)) = parse_node_json(node_str) {
            push_unique_tag(&mut tags, info.tag);
        }
    }

    // Existing rules reserve their target even while the node is absent. This
    // is the fail-closed fallback when node-bindings.json is missing or invalid:
    // a future same-name node must not silently inherit a dormant rule.
    for tag in ParsedCustomRules::parse(&config.custom_rules).outbound_tags() {
        push_unique_tag(&mut tags, tag);
    }
    for ap in &config.access_ports {
        if ap.outbound != "proxy" && ap.outbound != "direct" && ap.outbound != "reject" {
            push_unique_tag(&mut tags, ap.outbound.clone());
        }
    }

    let runtime_config_path = &state.runtime_paths.active_config;
    if let Ok(content) = tokio::fs::read_to_string(&runtime_config_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            for tag in runtime_config_node_tags(&json) {
                push_unique_tag(&mut tags, tag);
            }
        }
    }

    for tag in reserved_node_tags(state).await {
        push_unique_tag(&mut tags, tag);
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
        assert_eq!(
            second
                .available_multipliers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["1.3", "18"],
            "倍率选项也必须来自当前显示名，而非保留的旧 tag"
        );
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

    #[tokio::test]
    async fn generation_preserves_rule_semantics_across_tag_reservation_and_filtering() {
        use crate::models::RouteMode;
        use serde_json::json;

        let dormant = r#" { "domain_suffix" : "dormant.example", "outbound" : "gone-node" } "#;
        let direct = json!({"domain_suffix": "direct.example", "outbound": "direct"});
        // The builder must retain fields it does not mirror to DNS.
        let tcp_only = json!({
            "domain_suffix": "tcp.example", "network": "tcp", "outbound": "manual-a"
        });
        for route_mode in [RouteMode::Rule, RouteMode::Global] {
            let config = Config {
                nodes: vec![
                    r#"{"type":"trojan","tag":"manual-a","server":"manual.example.com","server_port":443,"password":"x"}"#.to_string(),
                ],
                custom_rules: vec![
                    dormant.to_string(),
                    direct.to_string(),
                    "{invalid".to_string(),
                    tcp_only.to_string(),
                    direct.to_string(),
                ],
                route_mode,
                ..Config::default()
            };
            let state = crate::test_support::app_state(config.clone());
            let outcome = super::build_prepared(
                &config,
                &state,
                vec![fetched_node("https://a", "gone-node")],
            )
            .await
            .unwrap();
            let generated: serde_json::Value = serde_json::from_slice(&outcome.bytes).unwrap();

            // Missing bindings must not let a new node take over an old rule.
            assert_eq!(
                generated["outbounds"][0]["outbounds"],
                json!(["manual-a", "gone-node (2)"])
            );
            assert_eq!(outcome.skipped_rules.len(), 1);
            assert_eq!(outcome.skipped_rules[0].raw, dormant);
            let rules = generated["route"]["rules"].as_array().unwrap();
            let custom_start = if route_mode == RouteMode::Rule { 5 } else { 2 };
            assert_eq!(
                &rules[custom_start..custom_start + 4],
                &[
                    json!({"domain_suffix":"dormant.example","action":"reject"}),
                    direct.clone(),
                    tcp_only.clone(),
                    direct.clone()
                ]
            );
            assert_eq!(
                rules.len(),
                if route_mode == RouteMode::Global {
                    6
                } else {
                    9
                }
            );

            let dns_rules = generated["dns"]["rules"].as_array().unwrap();
            let expected_dns = json!({
                "domain_suffix": "direct.example", "action": "route", "server": "local"
            });
            let custom_dns_start = usize::from(route_mode == RouteMode::Rule);
            assert_eq!(
                &dns_rules[custom_dns_start..custom_dns_start + 3],
                &[
                    json!({"domain_suffix":"dormant.example","action":"reject"}),
                    expected_dns.clone(),
                    expected_dns
                ]
            );
            assert_eq!(
                dns_rules.len(),
                if route_mode == RouteMode::Global {
                    3
                } else {
                    4
                }
            );
        }
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
    /// 使用固定失败结果避免依赖 loopback 拒连耗时。
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

    #[test]
    fn offline_manual_fallback_protects_subscription_runtime_and_cache() {
        use super::{decide_offline_manual_fallback, OfflineManualFallbackDecision};
        use serde_json::json;

        let manual_only_runtime = json!({
            "outbounds": [
                {"tag": "proxy", "type": "selector"},
                {"tag": "direct", "type": "direct"},
                {"tag": "manual-a", "type": "trojan"}
            ]
        });
        let mixed_runtime = json!({
            "outbounds": [
                {"tag": "manual-a", "type": "trojan"},
                {"tag": "sub-node", "type": "trojan"}
            ]
        });
        let manual = vec!["manual-a".to_string()];

        assert_eq!(
            decide_offline_manual_fallback(&manual, Some(&manual_only_runtime), true),
            OfflineManualFallbackDecision::RebuildFromManualNodes
        );
        assert_eq!(
            decide_offline_manual_fallback(&manual, Some(&mixed_runtime), false),
            OfflineManualFallbackDecision::RejectMissingSnapshot
        );
        assert_eq!(
            decide_offline_manual_fallback(&manual, None, true),
            OfflineManualFallbackDecision::RejectMissingSnapshot
        );
        assert_eq!(
            decide_offline_manual_fallback(&manual, None, false),
            OfflineManualFallbackDecision::RebuildFromManualNodes
        );
    }

    #[test]
    fn source_fetch_resolution_distinguishes_informational_empty_from_all_parse_failed() {
        use super::resolve_source_fetch;
        use crate::models::{SubscriptionFailureKind, SubscriptionState};
        use crate::services::subscription::FetchResult;
        use std::collections::HashMap;

        let url = "https://a.example/sub";
        let mut cached = HashMap::from([(
            subscription_source_id(url),
            vec![fetched_node(url, "cached-node")],
        )]);

        // Only informational entries -> authoritative empty list; cached nodes replaced.
        let info_only = resolve_source_fetch(
            url,
            Ok(FetchResult {
                node_names: vec![],
                outbounds: vec![],
                parse_errors: vec![],
                total_count: 2,
                filtered_info_count: 2,
            }),
            &mut cached,
        );
        assert!(info_only.succeeded);
        assert!(info_only.nodes.is_empty());
        assert_eq!(info_only.status.state, SubscriptionState::Ready);
        assert!(info_only
            .status
            .error
            .as_deref()
            .unwrap()
            .contains("2 informational entries filtered"));

        // Non-informational entries that all failed parsing -> Parse failure; cached nodes kept.
        let parse_failed = resolve_source_fetch(
            url,
            Ok(FetchResult {
                node_names: vec![],
                outbounds: vec![],
                parse_errors: vec!["bad proxy".into()],
                total_count: 3,
                filtered_info_count: 1,
            }),
            &mut cached,
        );
        assert!(!parse_failed.succeeded);
        assert_eq!(parse_failed.cached_nodes, 1);
        assert_eq!(parse_failed.nodes.len(), 1);
        assert_eq!(parse_failed.status.state, SubscriptionState::Failed);
        assert_eq!(
            parse_failed.status.failure_kind,
            Some(SubscriptionFailureKind::Parse)
        );
    }
}

#[cfg(test)]
#[path = "generate_recovery_tests.rs"]
mod recovery_tests;
