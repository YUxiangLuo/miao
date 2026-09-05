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
/// 缓存、快照或手动节点已让数据面就绪时，启动刷新失败不能就此停止：开机
/// 早期 DNS、DHCP 或默认路由可能在 20s 预算后才可用。失败后在保持当前
/// 配置运行的同时按 5–60s 退避继续单次拉取，直到成功。前台刷新会淘汰
/// 正在进行的旧请求；若前台仍未取到订阅节点，后台采用新 generation 继续恢复。
pub(super) async fn refresh_subscriptions_in_background(config: &Config, state: &Arc<AppState>) {
    let mut refresh_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    let mut retry = SubFetchRetry::Startup;
    let mut delay = STARTUP_RECOVERY_INITIAL_DELAY;

    loop {
        match background_subscription_refresh_once(config, state, refresh_generation, retry).await {
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

        info!(
            delay_secs = delay.as_secs(),
            "Startup subscription refresh will retry in the background"
        );
        tokio::time::sleep(delay).await;
        if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
            let Some(generation) =
                resume_after_foreground_refresh(config, state, refresh_generation).await
            else {
                return;
            };
            refresh_generation = generation;
        }
        delay = next_startup_recovery_delay(delay);
        // 后续已有外层持续退避；每轮只请求一次，避免永久坏订阅每分钟触发
        // 一整组启动预算内重试。
        retry = SubFetchRetry::None;
    }
}

/// 等待取代后台请求的前台事务结束。前台拿到并提交了订阅节点则后台完成；
/// 前台请求本身结束但仍无订阅节点时，只淘汰旧请求并采用新 generation 续跑。
async fn resume_after_foreground_refresh(
    startup_config: &Config,
    state: &Arc<AppState>,
    previous_generation: u64,
) -> Option<u64> {
    let _config_update = state.config_update.lock().await;
    let current_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    if current_generation == previous_generation {
        return Some(current_generation);
    }
    if !state.service_should_run.load(Ordering::Relaxed) {
        info!("Service stopped while foreground subscription refresh was running");
        return None;
    }
    let current = state.config.read().await;
    if current.subs != startup_config.subs || current.subs.is_empty() {
        info!("Subscriptions changed while foreground refresh superseded startup recovery");
        return None;
    }
    if state.sub_refresh_success_generation.load(Ordering::Relaxed) == current_generation {
        info!("Foreground subscription refresh succeeded; startup recovery is complete");
        return None;
    }

    info!("Foreground subscription refresh got no usable subscription nodes; resuming startup recovery");
    Some(current_generation)
}

async fn background_subscription_refresh_once(
    startup_config: &Config,
    state: &Arc<AppState>,
    refresh_generation: u64,
    retry: SubFetchRetry,
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
    state.set_runtime_phase(background_phase);
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
    if state.runtime_phase() == background_phase {
        state.set_runtime_phase(RuntimePhase::Ready);
    }
    step
}

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
