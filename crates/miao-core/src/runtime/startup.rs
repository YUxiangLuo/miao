use super::*;

pub(super) async fn initialize_runtime(
    config: Config,
    state: Arc<AppState>,
    extract_runtime: bool,
) {
    let Some(local_initialization) = state.begin_mutation() else {
        state.initializing.store(false, Ordering::Relaxed);
        return;
    };
    if extract_runtime && !extract_embedded_runtime_for_startup(&state).await {
        return;
    }

    // config_update 只覆盖本地启核。订阅 HTTP 一律在锁外拉取。
    let needs_background_refresh = {
        let _config_update = state.config_update.lock().await;
        let refresh = initialize_runtime_locked(&config, &state).await;
        // Ready publication confirms a real data plane. Empty configurations
        // and an explicit Stop are settled too; confirm under this lease/lock,
        // before a concurrent installer can create a new pending marker.
        confirm_upgrade_if_settled(&state).await;
        refresh
    };
    drop(local_initialization); // Never keep an admission lease over retry sleeps.

    if needs_background_refresh {
        refresh_subscriptions_in_background(&config, &state).await;
    } else {
        dispatch_recovery_outcome(&state, recover_data_plane_once(&state).await).await;
    }
    // Configuration can be removed while a failed startup fetch is in flight,
    // including between the initial checkpoint and the retry guard above.
    let Some(_confirmation) = state.begin_mutation() else {
        return;
    };
    let _config_update = state.config_update.lock().await;
    confirm_upgrade_if_settled(&state).await;
}

async fn extract_embedded_runtime_for_startup(state: &Arc<AppState>) -> bool {
    let extraction_generation = state.lifecycle.snapshot().generation;
    let _activity = state
        .lifecycle
        .activity(crate::state::lifecycle::RuntimeActivity::Extracting);
    let runtime_dir = state.runtime_paths.runtime_dir.clone();
    let extracted = tokio::task::spawn_blocking(move || extract_sing_box_to(&runtime_dir)).await;
    let warning = match extracted {
        Ok(Ok(_)) => return true,
        Ok(Err(err)) => {
            error!(error = %err, "Failed to prepare embedded sing-box runtime");
            format!("准备 sing-box 内核失败：{err}")
        }
        Err(err) => {
            error!(error = %err, "Embedded sing-box extraction task failed");
            format!("准备 sing-box 内核任务失败：{err}")
        }
    };
    *state.config_warning.lock().await = Some(warning);
    state
        .lifecycle
        .finish(extraction_generation, RuntimePhase::Failed);
    state.initializing.store(false, Ordering::Relaxed);
    false
}

async fn confirm_upgrade_if_settled(state: &Arc<AppState>) {
    if startup_is_settled(state).await && !state.lifecycle.is_shutting_down() {
        crate::services::version::mark_upgrade_healthy();
    }
}

async fn dispatch_recovery_outcome(state: &Arc<AppState>, outcome: RecoveryOutcome) {
    match outcome {
        RecoveryOutcome::Complete => {}
        RecoveryOutcome::RetrySubscriptions => retry_recovered_subscriptions(state).await,
        RecoveryOutcome::RetryDataPlane => retry_failed_startup(state).await,
        RecoveryOutcome::WaitForNodes => retry_waiting_for_nodes(state).await,
    }
}

async fn data_plane_is_ready(state: &AppState) -> bool {
    state.lifecycle.snapshot().ready && is_sing_box_running(state).await
}

pub(super) async fn startup_is_settled(state: &Arc<AppState>) -> bool {
    let runtime = state.lifecycle.snapshot();
    if runtime.ready || !runtime.should_run {
        return true;
    }
    let config = state.config.read().await;
    config.subs.is_empty() && config.nodes.is_empty()
}

pub(super) async fn should_retry_failed_startup(state: &Arc<AppState>) -> bool {
    let runtime = state.lifecycle.snapshot();
    if runtime.ready || !runtime.should_run {
        return false;
    }
    let config = state.config.read().await;
    !config.subs.is_empty() || !config.nodes.is_empty()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StartupCacheKind {
    Verified,
    /// Pre-manifest cache accepted once; still requires one background
    /// local reconciliation to prove all inputs and upgrade its manifest.
    Legacy,
}

/// Check cache provenance and runtime semantics before copying it into the
/// active slot.
pub(super) async fn prepare_compatible_startup_cache(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<StartupCacheKind> {
    let cache_kind = match cache_compatibility(state, config).await {
        CacheCompatibility::Verified => StartupCacheKind::Verified,
        CacheCompatibility::Legacy => StartupCacheKind::Legacy,
        CacheCompatibility::Incompatible(reason) => {
            return Err(AppError::message(format!(
                "Cached config provenance check failed: {reason}"
            )));
        }
    };

    // A legacy cache has no input manifest proving that its automatic members
    // were built under the requested cap. Its public tags may also predate the
    // current subscription display names. Reject it and let the next startup
    // tier rebuild from the local node snapshot instead of guessing.
    if cache_kind == StartupCacheKind::Legacy
        && !config.node_select.is_manual()
        && config.max_multiplier.is_some()
    {
        return Err(AppError::message(
            "Legacy automatic cache cannot prove max_multiplier compatibility",
        ));
    }

    // Validate and inspect the cache in place. A rejected cache must not
    // overwrite config.json, which is also the rollback snapshot source.
    validate_sing_box_config(state, &state.runtime_paths.config_cache).await?;
    let content = tokio::fs::read_to_string(&state.runtime_paths.config_cache)
        .await
        .map_err(|e| AppError::context("Failed to read cached config", e))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::context("Cached config is invalid JSON", e))?;
    if cache_kind == StartupCacheKind::Legacy
        && !runtime_config_matches_node_select(&json, config.node_select)
    {
        return Err(AppError::message(
            "Cached config does not match the effective node_select",
        ));
    }
    restore_config_from_cache(state).await?;
    Ok(cache_kind)
}

async fn publish_started_cache_runtime(
    config: &Config,
    state: &Arc<AppState>,
    cache_kind: StartupCacheKind,
) {
    if cache_kind == StartupCacheKind::Legacy {
        mark_legacy_cache_used(state).await;
    }
    publish_runtime_multiplier_options(config, state).await;
    publish_cached_rule_diagnostics(config, state).await;
    spawn_restore_last_proxy(state);
}

/// Restore a compatible cache into the active slot and start sing-box.
/// Used when startup recovery cannot activate a freshly generated config.
pub(super) async fn try_start_compatible_cache(config: &Config, state: &Arc<AppState>) -> bool {
    if !has_config_cache(state) {
        return false;
    }
    let cache_kind = match prepare_compatible_startup_cache(config, state).await {
        Ok(kind) => kind,
        Err(err) => {
            warn!(error = %err, "Cached config is not eligible during startup recovery");
            return false;
        }
    };
    if let Err(err) = start_sing_internal(state).await {
        warn!(error = %err, "Failed to start sing-box from cache during startup recovery");
        return false;
    }
    info!("sing-box started from cached config during startup recovery");
    publish_started_cache_runtime(config, state, cache_kind).await;
    true
}

fn local_startup_warning(config: &Config, outcome: &GenConfigOutcome) -> Option<String> {
    if !config.node_select.is_manual() && outcome.node_select.is_manual() {
        Some(REGION_FALLBACK.to_string())
    } else if !outcome.has_sub_nodes && !config.subs.is_empty() {
        Some(SUBS_REFRESHING_MANUAL.to_string())
    } else {
        None
    }
}

/// Install, validate and start a config rebuilt entirely from local node
/// material. A successful local start becomes the new verified exact cache;
/// subscription fetching can then happen in the background.
pub(super) async fn start_prepared_local_runtime(
    config: &Config,
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
    source: &'static str,
) -> AppResult<()> {
    let _activity = state
        .lifecycle
        .activity(crate::state::lifecycle::RuntimeActivity::Validating);
    install_prepared_runtime(state, outcome).await?;
    if let Err(err) = persist_effective_node_select(state, outcome.node_select).await {
        warn!(error = %err, "Failed to persist effective node_select after local startup rebuild");
    }

    start_sing_internal(state).await?;
    info!(source, "sing-box started from local startup material");

    save_config_cache(state).await;
    publish_generation_diagnostics(state, outcome).await;
    *state.config_warning.lock().await = local_startup_warning(config, outcome);
    spawn_restore_last_proxy(state);
    state.initializing.store(false, Ordering::Relaxed);
    Ok(())
}

/// Tier 1: returns `Some(needs_background_refresh)` when the cached config starts.
async fn try_start_cached_startup_runtime(config: &Config, state: &Arc<AppState>) -> Option<bool> {
    if !has_config_cache(state) {
        return None;
    }
    let _activity = state
        .lifecycle
        .activity(crate::state::lifecycle::RuntimeActivity::Validating);
    let cache_kind = match prepare_compatible_startup_cache(config, state).await {
        Ok(kind) => kind,
        Err(err) => {
            warn!(error = %err, "Cached config is not eligible for startup; regenerating");
            return None;
        }
    };
    if let Err(err) = start_sing_internal(state).await {
        error!(error = %err, "Failed to start sing-box from cache, fetching subscriptions");
        return None;
    }
    info!("sing-box started from cached config");
    publish_started_cache_runtime(config, state, cache_kind).await;
    state.initializing.store(false, Ordering::Relaxed);
    // A legacy cache still needs one local regeneration to prove all runtime
    // inputs and upgrade its manifest, even when there are no subscriptions.
    Some(cache_kind == StartupCacheKind::Legacy || !config.subs.is_empty())
}

/// Tier 2: rebuild from the last accepted subscription node snapshot (no network).
async fn try_start_snapshot_startup_runtime(config: &Config, state: &Arc<AppState>) -> bool {
    let Some(snapshot) = read_sub_nodes_snapshot(state).await else {
        return false;
    };
    if !snapshot.matches_subs(&config.subs) {
        warn!("Subscription node snapshot does not match current subscription list");
        return false;
    }
    info!("Rebuilding startup config from subscription node snapshot (no network)");
    let outcome = match gen_config_from_nodes(config, state, snapshot.to_fetched_nodes()).await {
        Ok(outcome) => outcome,
        Err(err) => {
            warn!(error = %err, "Failed to rebuild from subscription node snapshot");
            return false;
        }
    };
    if let Err(err) = start_prepared_local_runtime(config, state, &outcome, "node_snapshot").await {
        warn!(error = %err, "Failed to start from subscription node snapshot");
        return false;
    }
    true
}

/// Tier 3: build from configured manual nodes alone (no network).
async fn try_start_manual_startup_runtime(config: &Config, state: &Arc<AppState>) -> bool {
    if config.nodes.is_empty() {
        return false;
    }
    info!("Building startup config from manual nodes (no network)");
    let outcome = match gen_config_from_nodes(config, state, Vec::new()).await {
        Ok(outcome) => outcome,
        Err(err) => {
            warn!(error = %err, "No valid manual-node startup config available");
            return false;
        }
    };
    if let Err(err) = start_prepared_local_runtime(config, state, &outcome, "manual_nodes").await {
        warn!(error = %err, "Failed to start from manual nodes");
        return false;
    }
    true
}

/// 持锁执行的本地初始化。返回 true = 内核已用本地材料快速启动，订阅需改为
/// 后台刷新；false = 无配置或本地材料未能启动（订阅拉取在锁外进行）。
pub(super) async fn initialize_runtime_locked(config: &Config, state: &Arc<AppState>) -> bool {
    // Initialization may acquire the transaction lock after shutdown has stopped
    // the kernel. Do not replace that terminal status with a startup failure.
    if !state.lifecycle.snapshot().should_run {
        state.initializing.store(false, Ordering::Relaxed);
        return false;
    }

    if config.subs.is_empty() && config.nodes.is_empty() {
        info!("No subscriptions or nodes configured, waiting for onboarding");
        state
            .lifecycle
            .finish(state.lifecycle.snapshot().generation, RuntimePhase::Stopped);
        state.initializing.store(false, Ordering::Relaxed);
        return false;
    }

    // 快速通道：依次尝试精确缓存 → 本地订阅节点快照 → 本地手动节点（均不联网）。
    if let Some(needs_background_refresh) = try_start_cached_startup_runtime(config, state).await {
        return needs_background_refresh;
    }
    if try_start_snapshot_startup_runtime(config, state).await {
        return !config.subs.is_empty();
    }
    if try_start_manual_startup_runtime(config, state).await {
        return !config.subs.is_empty();
    }

    state
        .lifecycle
        .finish(state.lifecycle.snapshot().generation, RuntimePhase::Failed);
    state.initializing.store(false, Ordering::Relaxed);
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackgroundRefreshStep {
    Finished,
    Retry,
    Superseded,
    WaitForNodes,
}

/// Pure retry-budget and cadence policy for the background subscription owner.
struct BackgroundRetryCadence {
    slow_retry_interval: Duration,
    fetch_retry: SubFetchRetry,
    fast_delay: Duration,
    fast_retries: usize,
    waiting_for_nodes: bool,
    quiet: bool,
    next_wait: Duration,
}

impl BackgroundRetryCadence {
    fn new(slow_retry_interval: Duration, waiting_for_nodes: bool) -> Self {
        Self {
            slow_retry_interval,
            fetch_retry: if waiting_for_nodes {
                SubFetchRetry::None
            } else {
                SubFetchRetry::Startup
            },
            fast_delay: STARTUP_RECOVERY_INITIAL_DELAY,
            fast_retries: 0,
            waiting_for_nodes,
            quiet: false,
            next_wait: if waiting_for_nodes {
                slow_retry_interval
            } else {
                Duration::ZERO
            },
        }
    }

    fn mark_waiting_for_nodes(&mut self) {
        self.waiting_for_nodes = true;
    }

    /// Compute the next wait duration and optional quiet-mode warning given
    /// whether the kernel data plane is currently healthy.
    fn plan_next_wait(&mut self, runtime_ready: bool) -> (Duration, Option<&'static str>) {
        if self.waiting_for_nodes && runtime_ready {
            // A local edit may have repaired the runtime while this owner
            // slept. Keep the slow cadence, but no longer claim no nodes.
            self.waiting_for_nodes = false;
            self.fast_retries = STARTUP_BACKGROUND_FAST_RETRIES;
        }
        self.quiet = self.waiting_for_nodes
            || (self.fast_retries >= STARTUP_BACKGROUND_FAST_RETRIES && runtime_ready);
        if self.quiet {
            let warning = if self.waiting_for_nodes {
                crate::services::config::SUBS_WAITING_FOR_NODES
            } else {
                SUBS_RETRYING_SLOWLY
            };
            (self.slow_retry_interval, Some(warning))
        } else {
            self.fast_retries = self.fast_retries.saturating_add(1);
            (self.fast_delay, None)
        }
    }

    fn advance_after_scheduling(&mut self, scheduled_wait: Duration) {
        self.next_wait = scheduled_wait;
        self.fast_delay = next_startup_recovery_delay(self.fast_delay);
        // 后续由外层控制快速/低频重试；每轮只请求一次，不再重复首轮预算。
        self.fetch_retry = SubFetchRetry::None;
    }
}

/// 后台订阅刷新（快速通道启动后调用）。
/// 阶段 1 不持锁拉取订阅节点集：首次有绝对预算 20s，不阻塞面板写操作；
/// 阶段 2 持锁落地：拉取期间订阅列表被改过（面板编辑已按新配置自行应用）
/// 或服务被显式停止，则放弃本次刷新。
///
/// 首轮 20s 预算后最多再快速重试 4 次，跨过开机 DNS/DHCP 未就绪窗口。
/// 数据面已可用却仍取不到订阅时，保留当前配置并转为每 30 分钟静默重试，
/// 不再无限刷新面板的启动状态。数据面不可用时仍保留原来的恢复退避。
/// 前台操作不会重置快速重试次数；停服/改订阅会及时取消长时间等待。
pub(super) async fn refresh_subscriptions_in_background(config: &Config, state: &Arc<AppState>) {
    refresh_subscriptions_with_slow_retry_interval(config, state, SUBS_SLOW_RETRY_INTERVAL).await;
}

pub(super) async fn refresh_subscriptions_with_slow_retry_interval(
    config: &Config,
    state: &Arc<AppState>,
    slow_retry_interval: Duration,
) {
    refresh_subscriptions_with_initial_wait(config, state, slow_retry_interval, false).await;
}

pub(super) async fn refresh_subscriptions_with_initial_wait(
    config: &Config,
    state: &Arc<AppState>,
    slow_retry_interval: Duration,
    waiting_for_nodes: bool,
) {
    // This owner spans retry sleeps, but is NOT a mutation lease. Waiting
    // entrants must recheck completion rather than launch a duplicate fetch.
    let _owner = state.subscription_recovery.background.lock().await;
    let mut refresh_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    let mut cadence = BackgroundRetryCadence::new(slow_retry_interval, waiting_for_nodes);

    loop {
        // Also notice a foreground success that committed during our sleep.
        let Some(generation) =
            wait_for_background_retry(config, state, refresh_generation, cadence.next_wait).await
        else {
            return;
        };
        refresh_generation = generation;
        match background_subscription_refresh_once(
            config,
            state,
            refresh_generation,
            cadence.fetch_retry,
            cadence.quiet,
        )
        .await
        {
            BackgroundRefreshStep::Finished => return,
            BackgroundRefreshStep::Retry => {}
            BackgroundRefreshStep::WaitForNodes => cadence.mark_waiting_for_nodes(),
            BackgroundRefreshStep::Superseded => {
                let Some(generation) =
                    resume_after_foreground_refresh(config, state, refresh_generation).await
                else {
                    return;
                };
                refresh_generation = generation;
            }
        }

        let wait = {
            let _config_update = state.config_update.lock().await;
            let runtime_ready = data_plane_is_ready(state).await;
            let (wait, quiet_warning) = cadence.plan_next_wait(runtime_ready);
            if let Some(warning) = quiet_warning {
                // A newer foreground operation owns its warning and state.
                if state.sub_refresh_generation.load(Ordering::Relaxed) == refresh_generation
                    && state.lifecycle.snapshot().should_run
                    && !state.subscription_refresh.foreground_in_flight()
                    && !state.foreground_sub_refresh_succeeded(refresh_generation)
                {
                    *state.config_warning.lock().await = Some(warning.to_string());
                }
            }
            wait
        };
        state
            .subscription_refresh
            .wait_to_retry(refresh_generation, wait);
        info!(
            delay_secs = wait.as_secs(),
            quiet = cadence.quiet,
            "Startup subscription refresh will retry in the background"
        );
        cadence.advance_after_scheduling(wait);
    }
}

// Subscribe before checking the generation so stop/edit notifications cannot
// be lost. A foreground fetch starts outside config_update; waking must not
// launch a competing request or reset the original retry deadline.
async fn wait_for_background_retry(
    config: &Config,
    state: &Arc<AppState>,
    mut generation: u64,
    delay: Duration,
) -> Option<u64> {
    let deadline = tokio::time::Instant::now() + delay;
    loop {
        let cancelled = state.sub_refresh_cancel.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();
        let completed = state.subscription_refresh.changed.notified();
        tokio::pin!(completed);
        completed.as_mut().enable();
        generation = resume_after_foreground_refresh(config, state, generation).await?;
        if state.subscription_refresh.foreground_in_flight() {
            // The original timer may already have elapsed. Let the foreground
            // request finish and commit before deciding to start another fetch.
            tokio::select! {
                _ = completed => {},
                _ = cancelled => {},
            }
        } else {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if !remaining.is_zero() {
                // A foreground request reset the old task status, not this
                // deadline. Restore its remaining wait after a failed commit.
                state
                    .subscription_refresh
                    .wait_to_retry(generation, remaining);
            }
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => return Some(generation),
                _ = cancelled => {},
                _ = completed => {},
            }
        }
    }
}

/// 在事务锁下检查前台提交结果（不等待锁外 HTTP）。前台已提交有效订阅响应（含空列表）则
/// 后台完成；尚未成功时采用当前 generation，但不重置重试额度或等待期限。
async fn resume_after_foreground_refresh(
    startup_config: &Config,
    state: &Arc<AppState>,
    previous_generation: u64,
) -> Option<u64> {
    let _config_update = state.config_update.lock().await;
    let current_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    if !state.lifecycle.snapshot().should_run {
        info!("Service stopped while foreground subscription refresh was running");
        return None;
    }
    let current = state.config.read().await;
    if current.subs != startup_config.subs {
        info!("Subscriptions changed while foreground refresh superseded startup recovery");
        return None;
    }
    if state.sub_recovery_satisfied(current_generation) {
        info!("Subscription recovery already completed for this generation");
        return None;
    }

    if current_generation != previous_generation {
        info!("Foreground refresh has not committed a successful subscription response; resuming startup recovery");
    }
    Some(current_generation)
}

fn activated_startup_refresh_warning(
    current: &Config,
    effective_node_select: crate::models::NodeSelect,
    pending_subs: bool,
) -> Option<String> {
    if pending_subs {
        Some(ALL_SUBS_FAILED_KEEP_CACHE.to_string())
    } else if !current.node_select.is_manual() && effective_node_select.is_manual() {
        Some(REGION_FALLBACK.to_string())
    } else {
        None
    }
}

fn retry_if_subs_configured(current: &Config) -> BackgroundRefreshStep {
    if current.subs.is_empty() {
        BackgroundRefreshStep::Finished
    } else {
        BackgroundRefreshStep::Retry
    }
}

async fn check_background_refresh_preconditions(
    startup_config: &Config,
    state: &AppState,
    refresh_generation: u64,
) -> Result<Config, BackgroundRefreshStep> {
    if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
        return Err(BackgroundRefreshStep::Superseded);
    }
    if state.subscription_recovery.is_complete(refresh_generation) {
        return Err(BackgroundRefreshStep::Finished);
    }
    if !state.lifecycle.snapshot().should_run {
        info!("Service stopped before background refresh; skipping");
        return Err(BackgroundRefreshStep::Finished);
    }
    let before_fetch = state.config_with_preferences().await;
    if before_fetch.subs != startup_config.subs {
        info!("Subscriptions changed before background refresh; skipping");
        return Err(BackgroundRefreshStep::Finished);
    }
    Ok(before_fetch)
}

async fn check_background_commit_preconditions(
    startup_config: &Config,
    state: &AppState,
    refresh_generation: u64,
) -> Result<Config, BackgroundRefreshStep> {
    if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
        return Err(BackgroundRefreshStep::Superseded);
    }
    let current = state.config_with_preferences().await;
    if current.subs != startup_config.subs {
        info!(
            "Subscriptions changed during background refresh; skipping (panel edit already applied)"
        );
        return Err(BackgroundRefreshStep::Finished);
    }
    if !state.lifecycle.snapshot().should_run {
        info!("Service stopped during background refresh; skipping");
        return Err(BackgroundRefreshStep::Finished);
    }
    Ok(current)
}

async fn settle_background_refresh_attempt(
    current: &Config,
    state: &Arc<AppState>,
    refresh_generation: u64,
    fetch_report: crate::models::SubscriptionFetchReport,
    source: SubSource,
    quiet: bool,
) -> BackgroundRefreshStep {
    let result = refresh_subscriptions(current, state, RefreshPolicy::Startup, source).await;
    let step = match result {
        Ok(outcome) => match outcome.effect {
            RefreshEffect::Activated | RefreshEffect::SkippedUnchanged => {
                // Kernel activation and subscription acceptance are independent.
                // A manual/cache rebuild can succeed with every source offline.
                let pending = !current.subs.is_empty() && !fetch_report.accepted_response();
                save_config_cache(state).await;
                *state.config_warning.lock().await =
                    activated_startup_refresh_warning(current, outcome.node_select, pending);
                if outcome.effect == RefreshEffect::Activated {
                    spawn_restore_last_proxy(state);
                }
                if pending {
                    BackgroundRefreshStep::Retry
                } else {
                    state.subscription_recovery.complete(refresh_generation);
                    BackgroundRefreshStep::Finished
                }
            }
            RefreshEffect::KeptRunningOnTotalFailure => {
                warn!("{ALL_SUBS_FAILED_KEEP_CACHE}");
                *state.config_warning.lock().await = Some(ALL_SUBS_FAILED_KEEP_CACHE.to_string());
                retry_if_subs_configured(current)
            }
            RefreshEffect::KeptRunningOnValidationFailure => {
                error!("Refreshed config failed validation; keeping current config");
                *state.config_warning.lock().await = Some(REFRESH_VALIDATION_FAILED.to_string());
                retry_if_subs_configured(current)
            }
        },
        Err(err) if err.is_no_usable_nodes() && fetch_report.accepted_response() => {
            if state.lifecycle.snapshot().ready {
                // The response was authoritative; fetching it again is not a
                // network-recovery operation. Preserve availability, but do not
                // publish an unactivated empty snapshot or retry it as a failure.
                *state.config_warning.lock().await =
                    Some(crate::services::config::SUBS_NO_USABLE_KEEP_CACHE.to_string());
                state.subscription_recovery.complete(refresh_generation);
                BackgroundRefreshStep::Finished
            } else {
                *state.config_warning.lock().await =
                    Some(crate::services::config::SUBS_WAITING_FOR_NODES.to_string());
                BackgroundRefreshStep::WaitForNodes
            }
        }
        Err(err) => {
            warn!(error = %err, "Background subscription refresh failed");
            *state.config_warning.lock().await = Some(REFRESH_FAILED_KEEP_CACHE.to_string());
            retry_if_subs_configured(current)
        }
    };
    if quiet && step == BackgroundRefreshStep::Retry && state.lifecycle.snapshot().ready {
        *state.config_warning.lock().await = Some(SUBS_RETRYING_SLOWLY.to_string());
    }
    step
}

async fn background_subscription_refresh_once(
    startup_config: &Config,
    state: &Arc<AppState>,
    refresh_generation: u64,
    retry: SubFetchRetry,
    quiet: bool,
) -> BackgroundRefreshStep {
    let _attempt = state.subscription_recovery.attempt.lock().await;
    let Some(_mutation) = state.begin_mutation() else {
        return if state.lifecycle.is_shutting_down() {
            BackgroundRefreshStep::Finished
        } else {
            BackgroundRefreshStep::Retry
        };
    };
    let before_fetch =
        match check_background_refresh_preconditions(startup_config, state, refresh_generation)
            .await
        {
            Ok(config) => config,
            Err(step) => return step,
        };

    // Fetch activity is published independently of the proxy lifecycle.
    // Only validation/activation may change the runtime phase.
    let nodes = fetch_sub_nodes_if_current(&before_fetch, state, retry, refresh_generation).await;
    let fetch_report = nodes.report;

    let _config_update = state.config_update.lock().await;
    let current = match check_background_commit_preconditions(
        startup_config,
        state,
        refresh_generation,
    )
    .await
    {
        Ok(config) => config,
        Err(step) => return step,
    };

    settle_background_refresh_attempt(
        &current,
        state,
        refresh_generation,
        fetch_report,
        SubSource::Prefetched(nodes),
        quiet,
    )
    .await
}

pub(super) const STARTUP_BACKGROUND_FAST_RETRIES: usize = 4;
#[cfg(not(test))]
const SUBS_SLOW_RETRY_INTERVAL: Duration = Duration::from_secs(30 * 60);
#[cfg(test)]
pub(super) const SUBS_SLOW_RETRY_INTERVAL: Duration = Duration::from_millis(500);

#[cfg(not(test))]
const STARTUP_RECOVERY_INITIAL_DELAY: Duration = Duration::from_secs(5);
#[cfg(not(test))]
const STARTUP_RECOVERY_MAX_DELAY: Duration = Duration::from_secs(60);
#[cfg(test)]
const STARTUP_RECOVERY_INITIAL_DELAY: Duration = Duration::from_millis(20);
#[cfg(test)]
const STARTUP_RECOVERY_MAX_DELAY: Duration = Duration::from_millis(80);

pub(super) fn next_startup_recovery_delay(current: Duration) -> Duration {
    current
        .checked_mul(2)
        .unwrap_or(STARTUP_RECOVERY_MAX_DELAY)
        .min(STARTUP_RECOVERY_MAX_DELAY)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecoveryOutcome {
    Complete,
    RetryDataPlane,
    /// The kernel is ready, but no authoritative subscription response landed.
    RetrySubscriptions,
    /// An authoritative response cannot build a runtime. Await new nodes or a
    /// local configuration edit instead of repeatedly treating it as a failure.
    WaitForNodes,
}

enum DataPlaneAttemptPrep {
    ReadyToFetch { config: Config, generation: u64 },
    Resolved(RecoveryOutcome),
}

enum DataPlaneRefreshAttempt {
    Recovered(RecoveryOutcome),
    NeedsCacheFallback { waiting_for_nodes: bool },
}

async fn prepare_data_plane_recovery_attempt(state: &Arc<AppState>) -> DataPlaneAttemptPrep {
    let _config_update = state.config_update.lock().await;
    if !state.lifecycle.snapshot().should_run {
        return DataPlaneAttemptPrep::Resolved(RecoveryOutcome::Complete);
    }
    if state.subscription_refresh.foreground_in_flight() {
        return DataPlaneAttemptPrep::Resolved(RecoveryOutcome::RetryDataPlane);
    }
    // A foreground/local operation can repair the runtime while this
    // attempt waits for config_update. Do not retire its fresh success.
    if data_plane_is_ready(state).await {
        return DataPlaneAttemptPrep::Resolved(recovery_after_concurrent_change(state).await);
    }
    let generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    let generation = if state.foreground_sub_refresh_succeeded(generation) {
        // The last foreground commit may have deliberately left no nodes.
        // A new repair must not inherit that commit's completion marker;
        // later foreground successes still supersede this new generation.
        state.next_sub_refresh()
    } else {
        generation
    };
    state.subscription_recovery.invalidate();
    DataPlaneAttemptPrep::ReadyToFetch {
        config: state.config_with_preferences().await,
        generation,
    }
}

async fn revalidate_data_plane_commit(
    fetched_config: &Config,
    state: &Arc<AppState>,
    refresh_generation: u64,
) -> Result<Config, RecoveryOutcome> {
    if !state.lifecycle.snapshot().should_run {
        return Err(RecoveryOutcome::Complete);
    }
    let current = state.config_with_preferences().await;
    if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation
        || current.subs != fetched_config.subs
    {
        info!("Startup recovery was superseded; discarding stale fetch");
        return Err(recovery_after_concurrent_change(state).await);
    }
    if data_plane_is_ready(state).await {
        return Err(recovery_after_concurrent_change(state).await);
    }
    Ok(current)
}

async fn apply_data_plane_refresh_attempt(
    current: &Config,
    state: &Arc<AppState>,
    refresh_generation: u64,
    source: SubSource,
    accepted_response: bool,
    pending_subs: bool,
) -> DataPlaneRefreshAttempt {
    match refresh_subscriptions(current, state, RefreshPolicy::Startup, source).await {
        Ok(outcome) => match outcome.effect {
            RefreshEffect::Activated | RefreshEffect::SkippedUnchanged => {
                if data_plane_is_ready(state).await {
                    save_config_cache(state).await;
                    *state.config_warning.lock().await = activated_startup_refresh_warning(
                        current,
                        outcome.node_select,
                        pending_subs,
                    );
                    spawn_restore_last_proxy(state);
                    let recovery = if pending_subs {
                        RecoveryOutcome::RetrySubscriptions
                    } else {
                        state.subscription_recovery.complete(refresh_generation);
                        RecoveryOutcome::Complete
                    };
                    return DataPlaneRefreshAttempt::Recovered(recovery);
                }
                warn!("Startup recovery produced unchanged bytes without a ready data plane");
                DataPlaneRefreshAttempt::NeedsCacheFallback {
                    waiting_for_nodes: false,
                }
            }
            RefreshEffect::KeptRunningOnTotalFailure => {
                *state.config_warning.lock().await = Some(ALL_SUBS_FAILED_RETRY.to_string());
                DataPlaneRefreshAttempt::NeedsCacheFallback {
                    waiting_for_nodes: false,
                }
            }
            RefreshEffect::KeptRunningOnValidationFailure => {
                *state.config_warning.lock().await = Some(STARTUP_VALIDATION_RETRY.to_string());
                DataPlaneRefreshAttempt::NeedsCacheFallback {
                    waiting_for_nodes: false,
                }
            }
        },
        Err(err) => {
            warn!(error = %err, "Startup data-plane recovery attempt failed");
            let waiting_for_nodes = accepted_response && err.is_no_usable_nodes();
            *state.config_warning.lock().await = Some(DATA_PLANE_RETRYING.to_string());
            DataPlaneRefreshAttempt::NeedsCacheFallback { waiting_for_nodes }
        }
    }
}

async fn finish_data_plane_cache_fallback(
    current: &Config,
    state: &Arc<AppState>,
    pending_subs: bool,
    waiting_for_nodes: bool,
) -> RecoveryOutcome {
    if try_start_compatible_cache(current, state).await {
        if current.subs.is_empty() {
            return RecoveryOutcome::Complete;
        }
        *state.config_warning.lock().await = Some(
            if pending_subs {
                ALL_SUBS_FAILED_KEEP_CACHE
            } else {
                REFRESH_FAILED_KEEP_CACHE
            }
            .to_string(),
        );
        return RecoveryOutcome::RetrySubscriptions;
    }
    let runtime = state.lifecycle.snapshot();
    if !runtime.ready {
        state
            .lifecycle
            .finish(runtime.generation, RuntimePhase::Failed);
    }
    if waiting_for_nodes {
        *state.config_warning.lock().await =
            Some(crate::services::config::SUBS_WAITING_FOR_NODES.to_string());
        RecoveryOutcome::WaitForNodes
    } else {
        RecoveryOutcome::RetryDataPlane
    }
}

/// One bounded fetch/install attempt. Its registration spans network I/O and
/// commit, but not the outer retry sleep, so upgrade can drain without hanging
/// forever on an offline subscription.
pub(crate) async fn recover_data_plane_once(state: &Arc<AppState>) -> RecoveryOutcome {
    // Acquire before config_update and this attempt's mutation lease. An
    // admitted HTTP caller still retains its request lease while waiting.
    // Reuse the preceding attempt's committed result when ownership transfers.
    let _attempt = state.subscription_recovery.attempt.lock().await;
    let Some(_mutation) = state.begin_mutation() else {
        return if state.lifecycle.is_shutting_down() {
            RecoveryOutcome::Complete
        } else {
            RecoveryOutcome::RetryDataPlane
        };
    };
    if !should_retry_failed_startup(state).await {
        return recovery_after_concurrent_change(state).await;
    }

    let (config, refresh_generation) = match prepare_data_plane_recovery_attempt(state).await {
        DataPlaneAttemptPrep::ReadyToFetch { config, generation } => (config, generation),
        DataPlaneAttemptPrep::Resolved(outcome) => return outcome,
    };
    let nodes =
        fetch_sub_nodes_if_current(&config, state, SubFetchRetry::Startup, refresh_generation)
            .await;
    let accepted_response = nodes.report.accepted_response();
    let _config_update = state.config_update.lock().await;
    let current = match revalidate_data_plane_commit(&config, state, refresh_generation).await {
        Ok(current) => current,
        Err(outcome) => return outcome,
    };
    let pending = !current.subs.is_empty() && !accepted_response;

    match apply_data_plane_refresh_attempt(
        &current,
        state,
        refresh_generation,
        SubSource::Prefetched(nodes),
        accepted_response,
        pending,
    )
    .await
    {
        DataPlaneRefreshAttempt::Recovered(outcome) => outcome,
        DataPlaneRefreshAttempt::NeedsCacheFallback { waiting_for_nodes } => {
            finish_data_plane_cache_fallback(&current, state, pending, waiting_for_nodes).await
        }
    }
}

/// Another operation may repair the kernel without committing a subscription
/// response. Only its committed-success generation can retire network recovery.
async fn recovery_after_concurrent_change(state: &Arc<AppState>) -> RecoveryOutcome {
    let runtime = state.lifecycle.snapshot();
    let config = state.config.read().await;
    if !runtime.should_run || (config.subs.is_empty() && config.nodes.is_empty()) {
        return RecoveryOutcome::Complete;
    }
    if !runtime.ready {
        return RecoveryOutcome::RetryDataPlane;
    }
    let generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    if !config.subs.is_empty() && !state.sub_recovery_satisfied(generation) {
        RecoveryOutcome::RetrySubscriptions
    } else {
        RecoveryOutcome::Complete
    }
}

pub(crate) async fn retry_recovered_subscriptions(state: &Arc<AppState>) {
    let config = state.config_with_preferences().await;
    refresh_subscriptions_in_background(&config, state).await;
}

pub(crate) async fn retry_waiting_for_nodes(state: &Arc<AppState>) {
    let config = state.config_with_preferences().await;
    refresh_subscriptions_with_initial_wait(&config, state, SUBS_SLOW_RETRY_INTERVAL, true).await;
}

/// Keep repairing an unavailable initial data plane without blocking panel
/// mutations on subscription network I/O. Foreground subscription operations
/// advance `sub_refresh_generation`, so their result always wins.
pub(super) async fn retry_failed_startup(state: &Arc<AppState>) {
    let mut delay = STARTUP_RECOVERY_INITIAL_DELAY;
    loop {
        let generation = state.sub_refresh_generation.load(Ordering::Relaxed);
        state.subscription_refresh.wait_to_retry(generation, delay);
        tokio::time::sleep(delay).await;
        match recover_data_plane_once(state).await {
            RecoveryOutcome::Complete => return,
            RecoveryOutcome::RetrySubscriptions => {
                retry_recovered_subscriptions(state).await;
                return;
            }
            RecoveryOutcome::RetryDataPlane => {}
            RecoveryOutcome::WaitForNodes => {
                retry_waiting_for_nodes(state).await;
                return;
            }
        }
        delay = next_startup_recovery_delay(delay);
    }
}

#[cfg(test)]
mod activity_tests;
#[cfg(test)]
mod recovery_tests;
