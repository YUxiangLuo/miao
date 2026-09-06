use super::*;

pub(super) async fn initialize_runtime(
    config: Config,
    state: Arc<AppState>,
    extract_runtime: bool,
) {
    if extract_runtime {
        state.set_runtime_phase(RuntimePhase::Extracting);
        let runtime_dir = state.runtime_paths.runtime_dir.clone();
        let extracted =
            tokio::task::spawn_blocking(move || extract_sing_box_to(&runtime_dir)).await;
        match extracted {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                error!(error = %err, "Failed to prepare embedded sing-box runtime");
                *state.config_warning.lock().await = Some(format!("准备 sing-box 内核失败：{err}"));
                state
                    .runtime_ready
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Failed);
                state
                    .initializing
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
            Err(err) => {
                error!(error = %err, "Embedded sing-box extraction task failed");
                *state.config_warning.lock().await =
                    Some(format!("准备 sing-box 内核任务失败：{err}"));
                state
                    .runtime_ready
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Failed);
                state
                    .initializing
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
        }
    }

    state.set_runtime_phase(RuntimePhase::Initializing);
    // config_update 只覆盖本地启核。订阅 HTTP 一律在锁外拉取。
    let needs_background_refresh = {
        let _config_update = state.config_update.lock().await;
        initialize_runtime_locked(&config, &state).await
    };

    // An upgraded binary is healthy only after extraction and the expected
    // data plane have settled. Empty configurations have no data plane yet;
    // local cache/snapshot/manual starts have already passed readiness here.
    if startup_is_settled(&state).await {
        crate::services::version::mark_upgrade_healthy();
    }

    if needs_background_refresh {
        refresh_subscriptions_in_background(&config, &state).await;
    } else if should_retry_failed_startup(&state).await {
        let settled = recover_data_plane_once(&state).await;
        if !settled && should_retry_failed_startup(&state).await {
            retry_failed_startup(&state).await;
        }
    }
    // Configuration can be removed while a failed startup fetch is in flight,
    // including between the initial checkpoint and the retry guard above.
    if startup_is_settled(&state).await {
        crate::services::version::mark_upgrade_healthy();
    }
}

pub(super) async fn startup_is_settled(state: &Arc<AppState>) -> bool {
    if state.runtime_ready.load(Ordering::Relaxed)
        || !state.service_should_run.load(Ordering::Relaxed)
    {
        return true;
    }
    let config = state.config.read().await;
    config.subs.is_empty() && config.nodes.is_empty()
}

pub(super) async fn should_retry_failed_startup(state: &Arc<AppState>) -> bool {
    if state.runtime_ready.load(Ordering::Relaxed)
        || !state.service_should_run.load(Ordering::Relaxed)
    {
        return false;
    }
    let config = state.config.read().await;
    !config.subs.is_empty() || !config.nodes.is_empty()
}

/// Check cache provenance and runtime semantics before copying it into the
/// active slot. `Ok(true)` identifies the one-time legacy compatibility path.
pub(super) async fn prepare_compatible_startup_cache(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let compatibility = cache_compatibility(state, config).await;
    let legacy = match compatibility {
        CacheCompatibility::Verified => false,
        CacheCompatibility::Legacy => true,
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
    if legacy && !config.node_select.is_manual() && config.max_multiplier.is_some() {
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
    if legacy && !runtime_config_matches_node_select(&json, config.node_select) {
        return Err(AppError::message(
            "Cached config does not match the effective node_select",
        ));
    }
    restore_config_from_cache(state).await?;
    Ok(legacy)
}

/// Restore a compatible cache into the active slot and start sing-box.
/// Used when startup recovery cannot activate a freshly generated config.
pub(super) async fn try_start_compatible_cache(config: &Config, state: &Arc<AppState>) -> bool {
    if !has_config_cache(state) {
        return false;
    }
    match prepare_compatible_startup_cache(config, state).await {
        Ok(legacy_cache) => match start_sing_internal(state).await {
            Ok(()) => {
                info!("sing-box started from cached config during startup recovery");
                if legacy_cache {
                    mark_legacy_cache_used(state).await;
                }
                publish_runtime_multiplier_options(config, state).await;
                spawn_restore_last_proxy(state);
                true
            }
            Err(err) => {
                warn!(error = %err, "Failed to start sing-box from cache during startup recovery");
                false
            }
        },
        Err(err) => {
            warn!(error = %err, "Cached config is not eligible during startup recovery");
            false
        }
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
    state.set_runtime_phase(RuntimePhase::Validating);
    install_prepared_runtime(state, outcome).await?;
    if let Err(err) = persist_effective_node_select(state, outcome.node_select).await {
        warn!(error = %err, "Failed to persist effective node_select after local startup rebuild");
    }

    start_sing_internal(state).await?;
    info!(source, "sing-box started from local startup material");

    save_config_cache(state).await;
    publish_generation_diagnostics(state, outcome).await;
    *state.config_warning.lock().await =
        if !config.node_select.is_manual() && outcome.node_select.is_manual() {
            Some(REGION_FALLBACK.to_string())
        } else if !outcome.has_sub_nodes && !config.subs.is_empty() {
            Some(SUBS_REFRESHING_MANUAL.to_string())
        } else {
            None
        };
    spawn_restore_last_proxy(state);
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// 持锁执行的本地初始化。返回 true = 内核已用本地材料快速启动，订阅需改为
/// 后台刷新；false = 无配置或本地材料未能启动（订阅拉取在锁外进行）。
pub(super) async fn initialize_runtime_locked(config: &Config, state: &Arc<AppState>) -> bool {
    if config.subs.is_empty() && config.nodes.is_empty() {
        info!("No subscriptions or nodes configured, waiting for onboarding");
        state
            .runtime_ready
            .store(false, std::sync::atomic::Ordering::Relaxed);
        state.set_runtime_phase(RuntimePhase::Stopped);
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        return false;
    }

    // 快速通道：存在上次成功运行的缓存配置 → 先起内核（秒开），订阅改为后台刷新。
    // 缓存读取/校验/启动失败则继续本地 snapshot/manuals；本地材料全部失败才
    // 标记 Failed 并返回。HTTP 拉取在锁外 recover_data_plane_once 进行。
    if has_config_cache(state) {
        state.set_runtime_phase(RuntimePhase::Validating);
        match prepare_compatible_startup_cache(config, state).await {
            Ok(legacy_cache) => {
                match start_sing_internal(state).await {
                    Ok(()) => {
                        info!("sing-box started from cached config");
                        if legacy_cache {
                            mark_legacy_cache_used(state).await;
                        }
                        publish_runtime_multiplier_options(config, state).await;
                        spawn_restore_last_proxy(state);
                        state
                            .initializing
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        // A legacy cache still needs one local regeneration to
                        // prove all runtime inputs and upgrade its manifest,
                        // even when there are no subscriptions to fetch.
                        return legacy_cache || !config.subs.is_empty();
                    }
                    Err(err) => {
                        error!(error = %err, "Failed to start sing-box from cache, fetching subscriptions");
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "Cached config is not eligible for startup; regenerating");
            }
        }
    }

    // 本地 tier 2：精确缓存缺失/不兼容时，用上次订阅节点集按当前规则、
    // 路由模式和节点选择重新生成。订阅列表必须逐项一致，避免用错来源。
    if let Some(snapshot) = read_sub_nodes_snapshot(state).await {
        if snapshot.matches_subs(&config.subs) {
            info!("Rebuilding startup config from subscription node snapshot (no network)");
            match gen_config_from_nodes(config, state, snapshot.to_fetched_nodes()).await {
                Ok(outcome) => {
                    match start_prepared_local_runtime(config, state, &outcome, "node_snapshot")
                        .await
                    {
                        Ok(()) => return !config.subs.is_empty(),
                        Err(err) => {
                            warn!(error = %err, "Failed to start from subscription node snapshot")
                        }
                    }
                }
                Err(err) => {
                    warn!(error = %err, "Failed to rebuild from subscription node snapshot")
                }
            }
        } else {
            warn!("Subscription node snapshot does not match current subscription list");
        }
    }

    // 本地 tier 3：即使从未成功拉取过订阅，只要配置中还有有效手动节点，
    // 也先让数据面可用；订阅继续在后台刷新，成功后再无缝更新运行配置。
    if !config.nodes.is_empty() {
        info!("Building startup config from manual nodes (no network)");
        match gen_config_from_nodes(config, state, Vec::new()).await {
            Ok(outcome) => {
                match start_prepared_local_runtime(config, state, &outcome, "manual_nodes").await {
                    Ok(()) => return !config.subs.is_empty(),
                    Err(err) => warn!(error = %err, "Failed to start from manual nodes"),
                }
            }
            Err(err) => warn!(error = %err, "No valid manual-node startup config available"),
        }
    }

    state
        .runtime_ready
        .store(false, std::sync::atomic::Ordering::Relaxed);
    state.set_runtime_phase(RuntimePhase::Failed);
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackgroundRefreshStep {
    Finished,
    Retry,
    Superseded,
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
    let mut refresh_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    let mut retry = SubFetchRetry::Startup;
    let mut delay = STARTUP_RECOVERY_INITIAL_DELAY;
    let mut fast_retries = 0;
    let mut quiet = false;

    loop {
        // Also notice a foreground success that committed during our sleep.
        let Some(generation) =
            resume_after_foreground_refresh(config, state, refresh_generation).await
        else {
            return;
        };
        refresh_generation = generation;
        match background_subscription_refresh_once(config, state, refresh_generation, retry, quiet)
            .await
        {
            BackgroundRefreshStep::Finished => return,
            BackgroundRefreshStep::Retry => {}
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
            quiet = fast_retries >= STARTUP_BACKGROUND_FAST_RETRIES
                && state.runtime_ready.load(Ordering::Relaxed)
                && is_sing_box_running(state).await;
            if quiet {
                // A newer foreground operation owns its warning and state.
                if state.sub_refresh_generation.load(Ordering::Relaxed) == refresh_generation
                    && state.service_should_run.load(Ordering::Relaxed)
                    && (refresh_generation == 0
                        || state.sub_refresh_success_generation.load(Ordering::Relaxed)
                            != refresh_generation)
                {
                    *state.config_warning.lock().await = Some(SUBS_RETRYING_SLOWLY.to_string());
                }
                SUBS_SLOW_RETRY_INTERVAL
            } else {
                fast_retries = fast_retries.saturating_add(1);
                delay
            }
        };
        info!(
            delay_secs = wait.as_secs(),
            quiet, "Startup subscription refresh will retry in the background"
        );
        let Some(generation) =
            wait_for_background_retry(config, state, refresh_generation, wait).await
        else {
            return;
        };
        refresh_generation = generation;
        delay = next_startup_recovery_delay(delay);
        // 后续由外层控制快速/低频重试；每轮只请求一次，不再重复首轮预算。
        retry = SubFetchRetry::None;
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
        generation = resume_after_foreground_refresh(config, state, generation).await?;
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Some(generation),
            _ = cancelled => {}
        }
    }
}

/// 在事务锁下检查前台提交结果（不等待锁外 HTTP）。前台已提交订阅节点则
/// 后台完成；尚未成功时采用当前 generation，但不重置重试额度或等待期限。
async fn resume_after_foreground_refresh(
    startup_config: &Config,
    state: &Arc<AppState>,
    previous_generation: u64,
) -> Option<u64> {
    let _config_update = state.config_update.lock().await;
    let current_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    if !state.service_should_run.load(Ordering::Relaxed) {
        info!("Service stopped while foreground subscription refresh was running");
        return None;
    }
    let current = state.config.read().await;
    if current.subs != startup_config.subs {
        info!("Subscriptions changed while foreground refresh superseded startup recovery");
        return None;
    }
    if current_generation != 0
        && state.sub_refresh_success_generation.load(Ordering::Relaxed) == current_generation
    {
        info!("Foreground subscription refresh succeeded; startup recovery is complete");
        return None;
    }

    if current_generation != previous_generation {
        info!("Foreground subscription refresh got no usable subscription nodes; resuming startup recovery");
    }
    Some(current_generation)
}

async fn background_subscription_refresh_once(
    startup_config: &Config,
    state: &Arc<AppState>,
    refresh_generation: u64,
    retry: SubFetchRetry,
    quiet: bool,
) -> BackgroundRefreshStep {
    if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
        return BackgroundRefreshStep::Superseded;
    }
    if !state.service_should_run.load(Ordering::Relaxed) {
        info!("Service stopped before background refresh; skipping");
        return BackgroundRefreshStep::Finished;
    }
    let before_fetch = state.config_with_preferences().await;
    if before_fetch.subs != startup_config.subs {
        info!("Subscriptions changed before background refresh; skipping");
        return BackgroundRefreshStep::Finished;
    }

    let background_phase = if before_fetch.subs.is_empty() {
        RuntimePhase::Validating
    } else {
        RuntimePhase::RefreshingSubscriptions
    };
    // Low-frequency maintenance must not make an already working proxy look
    // as if it is starting/refreshing again. Real activation still sets phases.
    if !quiet || !state.runtime_ready.load(Ordering::Relaxed) {
        state.set_runtime_phase(background_phase);
    }
    let nodes = fetch_sub_nodes_if_current(&before_fetch, state, retry, refresh_generation).await;

    let _config_update = state.config_update.lock().await;
    if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
        return BackgroundRefreshStep::Superseded;
    }
    let current = state.config_with_preferences().await;
    if current.subs != startup_config.subs {
        info!(
            "Subscriptions changed during background refresh; skipping (panel edit already applied)"
        );
        return BackgroundRefreshStep::Finished;
    }
    if !state.service_should_run.load(Ordering::Relaxed) {
        info!("Service stopped during background refresh; skipping");
        return BackgroundRefreshStep::Finished;
    }

    let step = match refresh_subscriptions(
        &current,
        state,
        RefreshPolicy::Startup,
        SubSource::Prefetched(nodes),
    )
    .await
    {
        Ok(outcome) => match outcome.effect {
            RefreshEffect::Activated => {
                info!("sing-box activated refreshed subscriptions");
                save_config_cache(state).await;
                *state.config_warning.lock().await =
                    if !current.node_select.is_manual() && outcome.node_select.is_manual() {
                        Some(REGION_FALLBACK.to_string())
                    } else {
                        None
                    };
                spawn_restore_last_proxy(state);
                BackgroundRefreshStep::Finished
            }
            RefreshEffect::SkippedUnchanged => {
                // This also upgrades a one-time legacy cache marker to a
                // verified manifest after the freshly generated bytes match
                // the validated running cache.
                save_config_cache(state).await;
                *state.config_warning.lock().await =
                    if !current.node_select.is_manual() && outcome.node_select.is_manual() {
                        Some(REGION_FALLBACK.to_string())
                    } else {
                        None
                    };
                BackgroundRefreshStep::Finished
            }
            RefreshEffect::KeptRunningOnTotalFailure => {
                warn!("{ALL_SUBS_FAILED_KEEP_CACHE}");
                *state.config_warning.lock().await = Some(ALL_SUBS_FAILED_KEEP_CACHE.to_string());
                if current.subs.is_empty() {
                    BackgroundRefreshStep::Finished
                } else {
                    BackgroundRefreshStep::Retry
                }
            }
            RefreshEffect::KeptRunningOnValidationFailure => {
                error!("Refreshed config failed validation; keeping current config");
                *state.config_warning.lock().await = Some(REFRESH_VALIDATION_FAILED.to_string());
                if current.subs.is_empty() {
                    BackgroundRefreshStep::Finished
                } else {
                    BackgroundRefreshStep::Retry
                }
            }
        },
        Err(err) => {
            warn!(error = %err, "Background subscription refresh failed");
            *state.config_warning.lock().await = Some(REFRESH_FAILED_KEEP_CACHE.to_string());
            if current.subs.is_empty() {
                BackgroundRefreshStep::Finished
            } else {
                BackgroundRefreshStep::Retry
            }
        }
    };
    if quiet && step == BackgroundRefreshStep::Retry && state.runtime_ready.load(Ordering::Relaxed)
    {
        *state.config_warning.lock().await = Some(SUBS_RETRYING_SLOWLY.to_string());
    }
    if state.runtime_phase() == background_phase {
        state.set_runtime_phase(RuntimePhase::Ready);
    }
    step
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

/// One fetch-outside-lock + install-under-lock attempt. Returns true when the
/// data plane is ready or the service is no longer desired.
pub(crate) async fn recover_data_plane_once(state: &Arc<AppState>) -> bool {
    if !should_retry_failed_startup(state).await {
        return true;
    }
    if is_sing_box_running(state).await && state.runtime_ready.load(Ordering::Relaxed) {
        state.set_runtime_phase(RuntimePhase::Ready);
        return true;
    }

    let config = state.config_with_preferences().await;
    let refresh_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    if state.runtime_phase() == RuntimePhase::Failed {
        state.set_runtime_phase(if config.subs.is_empty() {
            RuntimePhase::Validating
        } else {
            RuntimePhase::FetchingSubscriptions
        });
    }
    let nodes =
        fetch_sub_nodes_if_current(&config, state, SubFetchRetry::Startup, refresh_generation)
            .await;

    let _config_update = state.config_update.lock().await;
    if !state.service_should_run.load(Ordering::Relaxed) {
        return true;
    }
    if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
        info!("Startup recovery was superseded by a foreground subscription operation");
        return state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await;
    }

    let current = state.config_with_preferences().await;
    if current.subs != config.subs {
        info!("Subscriptions changed during startup recovery; discarding stale fetch");
        return state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await;
    }
    if state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await {
        state.set_runtime_phase(RuntimePhase::Ready);
        return true;
    }

    match refresh_subscriptions(
        &current,
        state,
        RefreshPolicy::Startup,
        SubSource::Prefetched(nodes),
    )
    .await
    {
        Ok(outcome) => match outcome.effect {
            RefreshEffect::Activated => {
                info!(?outcome.runtime_update, "Initial data plane recovered in the background");
                save_config_cache(state).await;
                *state.config_warning.lock().await =
                    if !current.node_select.is_manual() && outcome.node_select.is_manual() {
                        Some(REGION_FALLBACK.to_string())
                    } else {
                        None
                    };
                spawn_restore_last_proxy(state);
                return true;
            }
            RefreshEffect::SkippedUnchanged => {
                if state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await {
                    save_config_cache(state).await;
                    state.set_runtime_phase(RuntimePhase::Ready);
                    return true;
                }
                warn!("Startup recovery produced unchanged bytes without a ready data plane");
            }
            RefreshEffect::KeptRunningOnTotalFailure => {
                warn!("Startup recovery still cannot fetch any subscription nodes");
                *state.config_warning.lock().await = Some(ALL_SUBS_FAILED_RETRY.to_string());
            }
            RefreshEffect::KeptRunningOnValidationFailure => {
                error!("Startup recovery generated an invalid configuration");
                *state.config_warning.lock().await = Some(STARTUP_VALIDATION_RETRY.to_string());
            }
        },
        Err(err) => {
            warn!(error = %err, "Startup data-plane recovery attempt failed");
            *state.config_warning.lock().await = Some(DATA_PLANE_RETRYING.to_string());
        }
    }
    if try_start_compatible_cache(&current, state).await {
        return true;
    }
    if !state.runtime_ready.load(Ordering::Relaxed) {
        state.set_runtime_phase(RuntimePhase::Failed);
    }
    false
}

/// Keep repairing an unavailable initial data plane without blocking panel
/// mutations on subscription network I/O. Foreground subscription operations
/// advance `sub_refresh_generation`, so their result always wins.
pub(super) async fn retry_failed_startup(state: &Arc<AppState>) {
    let mut delay = STARTUP_RECOVERY_INITIAL_DELAY;
    loop {
        tokio::time::sleep(delay).await;
        if recover_data_plane_once(state).await {
            return;
        }
        delay = next_startup_recovery_delay(delay);
    }
}
