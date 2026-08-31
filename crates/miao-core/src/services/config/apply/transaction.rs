use super::*;

pub(in crate::services::config) async fn regenerate_and_restart_runtime(
    config: &Config,
    state: &Arc<AppState>,
    policy: RefreshPolicy,
    source: SubSource,
) -> AppResult<RefreshOutcome> {
    let outcome = refresh_subscriptions(config, state, policy, source).await?;
    if outcome.generated.is_none() {
        return Err(AppError::message(
            "Runtime refresh kept the previous configuration",
        ));
    }
    Ok(outcome)
}

pub async fn regenerate_preserving_service_state(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<RuntimeUpdate> {
    // Effective state may be manual after a regional fallback. Every explicit
    // refresh retries the requested strategy rather than making that fallback
    // sticky until the next process restart. Keep the effective snapshot for
    // rollback if the preferred regeneration cannot commit.
    let previous_config = config;
    let preferred_config = state.overlay_preferences(config).await;
    let config = &preferred_config;
    // This is an explicit foreground refresh. Any startup fetch that began
    // earlier with the same subscription URLs must not publish after it.
    let refresh_generation = state.sub_refresh_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let should_run = state.service_should_run.load(Ordering::Relaxed);
    state.set_runtime_phase(RuntimePhase::ApplyingConfig);

    if config_apply_mode(config, should_run) == ConfigApplyMode::Clear {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(RuntimeUpdate::None);
    }

    // Snapshot bytes before this refresh so rollback can restore without a fetch.
    let snapshot = snapshot_runtime_config(state).await;
    let bindings_snapshot = read_file_snapshot(&state.runtime_paths.node_bindings).await?;

    let (runtime_update, has_sub_nodes) = if should_run {
        match regenerate_and_restart_runtime(config, state, RefreshPolicy::Manual, SubSource::Fetch)
            .await
        {
            Ok(refresh) => {
                let outcome = refresh
                    .generated
                    .as_ref()
                    .expect("checked generated outcome");
                if let Err(commit_err) = commit_foreground_refresh(config, state, outcome).await {
                    error!(error = %commit_err, "Foreground runtime refresh could not commit effective preferences; restoring previous runtime state");
                    return Err(rollback_failed_foreground_commit(
                        previous_config,
                        state,
                        true,
                        snapshot.as_deref(),
                        bindings_snapshot.as_deref(),
                        refresh.runtime_update.updated(),
                        commit_err,
                    )
                    .await);
                }
                let runtime_update = if refresh.effect == RefreshEffect::Activated {
                    finalize_started_config(config, state, outcome.has_sub_nodes).await;
                    refresh.runtime_update
                } else {
                    update_config_warning(config, state, outcome.has_sub_nodes).await;
                    state.runtime_ready.store(true, Ordering::Relaxed);
                    state.set_runtime_phase(RuntimePhase::Ready);
                    RuntimeUpdate::None
                };
                (runtime_update, outcome.has_sub_nodes)
            }
            Err(err) => {
                // Candidate validation never replaces config.json. Activation
                // may have, so rewind runtime + bindings before surfacing err.
                error!(error = %err, "Failed to refresh subscriptions, restoring previous runtime state");
                let restore = restore_after_apply_failure(
                    previous_config,
                    state,
                    true,
                    snapshot.as_deref(),
                    bindings_snapshot.as_deref(),
                    false,
                )
                .await;
                return match restore {
                    Ok(()) => Err(err),
                    Err(restore_err) => Err(AppError::message(format!(
                        "Failed to refresh subscriptions: {}. Runtime rollback failed: {}",
                        err, restore_err
                    ))),
                };
            }
        }
    } else {
        let has_sub_nodes = match regenerate_without_restart_runtime(
            config,
            state,
            SubSource::Fetch,
        )
        .await
        {
            Ok(outcome) => {
                if let Err(commit_err) = commit_foreground_refresh(config, state, &outcome).await {
                    error!(error = %commit_err, "Stopped runtime refresh could not commit effective preferences; restoring previous runtime files");
                    return Err(rollback_failed_foreground_commit(
                        previous_config,
                        state,
                        false,
                        snapshot.as_deref(),
                        bindings_snapshot.as_deref(),
                        false,
                        commit_err,
                    )
                    .await);
                }
                update_config_warning(config, state, outcome.has_sub_nodes).await;
                state.set_runtime_phase(RuntimePhase::Stopped);
                outcome.has_sub_nodes
            }
            Err(err) => {
                error!(error = %err, "Failed to regenerate config, restoring previous runtime config");
                let _ = restore_after_apply_failure(
                    previous_config,
                    state,
                    false,
                    snapshot.as_deref(),
                    bindings_snapshot.as_deref(),
                    false,
                )
                .await;
                return Err(err);
            }
        };
        (RuntimeUpdate::None, has_sub_nodes)
    };

    if has_sub_nodes {
        state
            .sub_refresh_success_generation
            .store(refresh_generation, Ordering::Relaxed);
    }
    Ok(runtime_update)
}

pub(in crate::services::config) async fn finalize_started_config(
    config: &Config,
    state: &Arc<AppState>,
    has_sub_nodes: bool,
) {
    update_config_warning(config, state, has_sub_nodes).await;

    spawn_restore_last_proxy(state);
}

async fn commit_foreground_refresh(
    config: &Config,
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
) -> AppResult<()> {
    // Persistence is the commit point. Do not publish snapshots or diagnostics
    // until it succeeds, otherwise an API error could leave observable state
    // describing runtime bytes that are about to be rolled back.
    persist_effective_node_select(state, outcome.node_select).await?;
    record_fresh_snapshot(config, state, outcome).await;
    publish_generation_diagnostics(state, outcome).await;
    Ok(())
}

async fn rollback_failed_foreground_commit(
    config: &Config,
    state: &Arc<AppState>,
    should_run: bool,
    runtime_snapshot: Option<&[u8]>,
    bindings_snapshot: Option<&[u8]>,
    force_restart: bool,
    commit_err: AppError,
) -> AppError {
    match restore_after_apply_failure(
        config,
        state,
        should_run,
        runtime_snapshot,
        bindings_snapshot,
        force_restart,
    )
    .await
    {
        Ok(()) => AppError::context(
            "Failed to commit refreshed configuration; restored previous runtime config",
            commit_err,
        ),
        Err(restore_err) => AppError::message(format!(
            "Failed to commit refreshed configuration: {}. Runtime rollback failed: {}",
            commit_err, restore_err
        )),
    }
}

async fn update_config_warning(config: &Config, state: &Arc<AppState>, has_sub_nodes: bool) {
    save_config_cache(state).await;

    let effective = state.config.read().await.node_select;
    *state.config_warning.lock().await = if !config.node_select.is_manual() && effective.is_manual()
    {
        Some(REGION_FALLBACK.to_string())
    } else if has_sub_nodes {
        None
    } else if !config.subs.is_empty() {
        Some(ALL_SUBS_FAILED.to_string())
    } else {
        None
    };
}

pub(in crate::services::config) async fn regenerate_without_restart_runtime(
    config: &Config,
    state: &Arc<AppState>,
    source: SubSource,
) -> AppResult<GenConfigOutcome> {
    let outcome = match source {
        SubSource::Fetch => gen_config(config, state, SubFetchRetry::None).await,
        SubSource::SnapshotOrFetch => gen_config_from_snapshot(config, state).await,
        SubSource::Prefetched(nodes) => gen_config_from_nodes(config, state, nodes).await,
    }
    .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    install_prepared_runtime(state, &outcome)
        .await
        .map_err(|e| AppError::context("Config validation or installation failed", e))?;

    Ok(outcome)
}

fn has_configured_sources(config: &Config) -> bool {
    !config.subs.is_empty() || !config.nodes.is_empty()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::services::config) enum ConfigApplyMode {
    Clear,
    Restart,
    RegenerateOnly,
}

pub(in crate::services::config) fn config_apply_mode(
    config: &Config,
    should_run: bool,
) -> ConfigApplyMode {
    if !has_configured_sources(config) {
        ConfigApplyMode::Clear
    } else if should_run {
        ConfigApplyMode::Restart
    } else {
        ConfigApplyMode::RegenerateOnly
    }
}

async fn remove_runtime_config_files_at(
    runtime_config_path: &Path,
    cache_path: &Path,
    sub_nodes_path: &Path,
) {
    for path in [runtime_config_path, cache_path, sub_nodes_path] {
        if let Err(err) = tokio::fs::remove_file(path).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(path = ?path, error = %err, "Failed to remove stale runtime config");
            }
        }
    }
}

async fn remove_file_if_present(path: &Path) {
    if let Err(err) = tokio::fs::remove_file(path).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(path = ?path, error = %err, "Failed to remove stale runtime config");
        }
    }
}

async fn remove_runtime_config_files(state: &AppState) {
    remove_runtime_config_files_at(
        &state.runtime_paths.active_config,
        &state.runtime_paths.config_cache,
        &state.runtime_paths.sub_nodes_snapshot,
    )
    .await;
    remove_file_if_present(&state.runtime_paths.cache_manifest).await;
    // Bindings live next to config.yaml, not in tmpfs. Only Clear (no remaining
    // sources) drops them; a no-usable-nodes persist must keep tag identity.
    remove_file_if_present(&state.runtime_paths.node_bindings).await;
}

async fn clear_runtime_config(state: &Arc<AppState>) {
    // Also drops node-bindings.json: with no remaining nodes a later add
    // must not inherit ghost tag reservations.
    remove_runtime_config_files(state).await;
    state.sub_status.lock().await.clear();
    *state.config_warning.lock().await = None;
}

pub(in crate::services::config) fn no_usable_nodes_warning(config: &Config) -> String {
    if config.subs.is_empty() {
        NO_USABLE_MANUAL.to_string()
    } else {
        NO_USABLE_SUBS.to_string()
    }
}

pub(in crate::services::config) async fn persist_config_without_usable_nodes_at(
    state: &Arc<AppState>,
    persisted_config: Config,
    runtime_config_path: &Path,
    cache_path: &Path,
    sub_nodes_path: &Path,
) -> AppResult<()> {
    save_config_layered(state, &persisted_config).await?;
    stop_sing_internal(state).await;
    remove_runtime_config_files_at(runtime_config_path, cache_path, sub_nodes_path).await;
    *state.config.write().await = persisted_config.clone();
    *state.config_warning.lock().await = Some(no_usable_nodes_warning(&persisted_config));
    Ok(())
}

async fn persist_config_without_usable_nodes(
    state: &Arc<AppState>,
    persisted_config: Config,
) -> AppResult<()> {
    persist_config_without_usable_nodes_at(
        state,
        persisted_config,
        &state.runtime_paths.active_config,
        &state.runtime_paths.config_cache,
        &state.runtime_paths.sub_nodes_snapshot,
    )
    .await
}

async fn restore_previous_config(
    old_config: &Config,
    state: &Arc<AppState>,
    should_run: bool,
    snapshot: Option<&[u8]>,
    force_restart: bool,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(());
    }

    if should_run && force_restart {
        restart_with_previous_config(old_config, state, snapshot).await
    } else if should_run {
        restore_previous_running_config(old_config, state, snapshot).await
    } else {
        restore_previous_stopped_config(old_config, state, snapshot).await
    }
}

async fn restore_after_apply_failure(
    old_config: &Config,
    state: &Arc<AppState>,
    should_run: bool,
    runtime_snapshot: Option<&[u8]>,
    bindings_snapshot: Option<&[u8]>,
    force_restart: bool,
) -> AppResult<()> {
    // Bindings commit together with config.json. Restore them even if runtime
    // recovery failed so the next successful transaction starts from old state.
    let runtime_restore = restore_previous_config(
        old_config,
        state,
        should_run,
        runtime_snapshot,
        force_restart,
    )
    .await;
    let bindings_restore =
        restore_file_snapshot(&state.runtime_paths.node_bindings, bindings_snapshot).await;

    match (runtime_restore, bindings_restore) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(runtime_err), Ok(())) => Err(runtime_err),
        (Ok(()), Err(bindings_err)) => Err(bindings_err),
        (Err(runtime_err), Err(bindings_err)) => Err(AppError::message(format!(
            "Runtime rollback failed: {runtime_err}. Bindings rollback failed: {bindings_err}"
        ))),
    }
}

pub async fn apply_config_change(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
) -> AppResult<ConfigApplyEffect> {
    // Handlers clone the effective config, whose strategy may be a temporary
    // manual fallback. Configuration changes must regenerate with the user's
    // requested strategy instead of accidentally extending that fallback.
    let preferred_new_config = state.overlay_preferences(new_config).await;
    let new_config = &preferred_new_config;
    let should_run = state.service_should_run.load(Ordering::Relaxed);
    let apply_mode = config_apply_mode(new_config, should_run);
    let previous_phase = state.runtime_phase();

    if apply_mode == ConfigApplyMode::Clear {
        state.set_runtime_phase(RuntimePhase::ApplyingConfig);
        if let Err(err) = save_config_layered(state, new_config).await {
            state.set_runtime_phase(previous_phase);
            return Err(err);
        }
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        *state.config.write().await = new_config.clone();
        *state.skipped_rules.lock().await = Vec::new();
        *state.available_multipliers.write().await = Vec::new();
        return Ok(ConfigApplyEffect::Cleared);
    }

    // 回滚 tier 1 材料：变更前正在运行/最近可用的运行时配置字节（config.json）
    let snapshot = snapshot_runtime_config(state).await;
    // node-bindings.json 与运行时配置共同提交；旧值读不出时不要开始一个
    // 无法完整回滚的事务。
    let bindings_snapshot = read_file_snapshot(&state.runtime_paths.node_bindings).await?;
    // 订阅列表没变就是本地语义变更（节点选择/路由模式/规则/手动节点），走快照零网络重建
    let source = sub_source_for(old_config, new_config);
    if matches!(&source, SubSource::Fetch) {
        state.sub_refresh_generation.fetch_add(1, Ordering::Relaxed);
    }
    state.set_runtime_phase(RuntimePhase::ApplyingConfig);

    let apply_result: AppResult<(GenConfigOutcome, RuntimeUpdate)> = match apply_mode {
        ConfigApplyMode::Restart => {
            regenerate_and_restart_runtime(new_config, state, RefreshPolicy::ManualInApply, source)
                .await
                .and_then(|refresh| {
                    let runtime_update = refresh.runtime_update;
                    refresh
                        .generated
                        .map(|outcome| (outcome, runtime_update))
                        .ok_or_else(|| {
                            AppError::message("Runtime refresh kept the previous configuration")
                        })
                })
        }
        ConfigApplyMode::RegenerateOnly => {
            regenerate_without_restart_runtime(new_config, state, source)
                .await
                .map(|outcome| (outcome, RuntimeUpdate::None))
        }
        ConfigApplyMode::Clear => unreachable!("clear mode handled above"),
    };

    match apply_result {
        Ok((outcome, runtime_update)) => {
            let persisted_new_config = Config {
                node_select: outcome.node_select,
                ..new_config.clone()
            };
            match save_config_layered(state, &persisted_new_config).await {
                Ok(()) => {
                    *state.config.write().await = persisted_new_config;
                    record_fresh_snapshot(new_config, state, &outcome).await;
                    publish_generation_diagnostics(state, &outcome).await;
                    if should_run {
                        if runtime_update.updated() {
                            finalize_started_config(new_config, state, outcome.has_sub_nodes).await;
                        } else {
                            update_config_warning(new_config, state, outcome.has_sub_nodes).await;
                            state.runtime_ready.store(true, Ordering::Relaxed);
                            state.set_runtime_phase(RuntimePhase::Ready);
                        }
                    } else {
                        update_config_warning(new_config, state, outcome.has_sub_nodes).await;
                        state.set_runtime_phase(RuntimePhase::Stopped);
                    }
                    Ok(if should_run {
                        if runtime_update.updated() {
                            ConfigApplyEffect::Activated(runtime_update)
                        } else {
                            ConfigApplyEffect::Unchanged
                        }
                    } else {
                        ConfigApplyEffect::Regenerated
                    })
                }
                Err(save_err) => {
                    error!(error = %save_err, "Runtime config applied but persistent config write failed, attempting runtime rollback");
                    match restore_after_apply_failure(
                        old_config,
                        state,
                        should_run,
                        snapshot.as_deref(),
                        bindings_snapshot.as_deref(),
                        runtime_update.updated(),
                    )
                    .await
                    {
                        Ok(()) => Err(AppError::context(
                            "Failed to persist config change; restored previous runtime config",
                            save_err,
                        )),
                        Err(rollback_err) => Err(AppError::message(format!(
                            "Failed to persist config change: {}. Runtime rollback failed: {}",
                            save_err, rollback_err
                        ))),
                    }
                }
            }
        }
        Err(apply_err) if apply_err.is_no_usable_nodes() => {
            // 有本地可用材料（运行时快照/cache/节点集快照）时，订阅全失败不再停核清场：
            // 回滚到变更前状态，把订阅故障作为普通变更失败报给用户
            if snapshot.is_some() || has_config_cache(state) || has_sub_nodes_snapshot(state) {
                warn!(error = %apply_err, "All subscriptions failed during config change; keeping previous runtime state");
                match restore_after_apply_failure(
                    old_config,
                    state,
                    should_run,
                    snapshot.as_deref(),
                    bindings_snapshot.as_deref(),
                    false,
                )
                .await
                {
                    Ok(()) => Err(AppError::context(
                        "所有订阅获取失败，已保留当前运行配置",
                        apply_err,
                    )),
                    Err(rollback_err) => Err(AppError::message(format!(
                        "所有订阅获取失败: {}. 恢复先前运行状态失败: {}",
                        apply_err, rollback_err
                    ))),
                }
            } else {
                // 本地没有任何可用材料（新装/清场后）：没有可回退的状态，维持落盘+停核
                warn!(error = %apply_err, "Config change left no usable nodes; persisting it and stopping sing-box");
                persist_config_without_usable_nodes(state, new_config.clone())
                    .await
                    .map(|()| ConfigApplyEffect::Cleared)
            }
        }
        Err(apply_err) => {
            error!(error = %apply_err, "Failed to apply runtime config change, attempting runtime rollback");
            match restore_after_apply_failure(
                old_config,
                state,
                should_run,
                snapshot.as_deref(),
                bindings_snapshot.as_deref(),
                false,
            )
            .await
            {
                Ok(()) => Err(AppError::context(
                    "Failed to apply config change; restored previous runtime config",
                    apply_err,
                )),
                Err(rollback_err) => Err(AppError::message(format!(
                    "Failed to apply config change: {}. Runtime rollback failed: {}",
                    apply_err, rollback_err
                ))),
            }
        }
    }
}

/// Caller must not already hold `config_update`.
/// Returns `(previous, runtime_update)` observed under that lock.
/// 配置变更的错误两分：闭包拒绝（请求校验失败，调用方映射 400）与事务失败（500）。
#[derive(Debug)]
pub enum ConfigMutationError {
    Rejected(String),
    Apply(AppError),
}

impl std::fmt::Display for ConfigMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) => write!(f, "{message}"),
            Self::Apply(err) => write!(f, "{err}"),
        }
    }
}

/// 「改配置 → 落盘 → 热应用」事务的统一入口：read-modify-write 整体在
/// `config_update` 锁内完成——变更闭包基于锁内最新配置克隆计算，并发请求
/// 不再可能基于锁外快照互相覆盖（丢失更新）。闭包返回 Err 则事务不开始；
/// 变更后配置无变化则跳过事务（幂等免费）。
async fn apply_config_mutation(
    state: &Arc<AppState>,
    mutate: impl FnOnce(&mut Config) -> Result<(), String>,
) -> Result<RuntimeUpdate, ConfigMutationError> {
    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = state.overlay_preferences(&old_config).await;
    mutate(&mut new_config).map_err(ConfigMutationError::Rejected)?;
    if new_config == old_config {
        return Ok(RuntimeUpdate::None);
    }
    apply_config_change(state, &old_config, &new_config)
        .await
        .map(|effect| effect.runtime_update())
        .map_err(ConfigMutationError::Apply)
}

pub async fn apply_route_mode(
    state: &Arc<AppState>,
    route_mode: RouteMode,
) -> Result<(RouteMode, RuntimeUpdate), ConfigMutationError> {
    let mut previous = RouteMode::default();
    let update = apply_config_mutation(state, |config| {
        previous = config.route_mode;
        config.route_mode = route_mode;
        Ok(())
    })
    .await?;
    Ok((previous, update))
}

/// Caller must not already hold `config_update`.
/// 禁用集变更闭包在锁内基于最新配置执行：增删条目与空池校验都是原子的。
pub async fn apply_disabled_nodes(
    state: &Arc<AppState>,
    mutate: impl FnOnce(&mut Config) -> Result<(), String>,
) -> Result<RuntimeUpdate, ConfigMutationError> {
    apply_config_mutation(state, mutate).await
}

/// Caller must not already hold `config_update`.
/// Returns `(previous, effective, runtime_update)` observed under that lock.
/// `effective` may fall back to manual when the region has no nodes.
pub async fn apply_node_select(
    state: &Arc<AppState>,
    node_select: NodeSelect,
) -> Result<(NodeSelect, NodeSelect, RuntimeUpdate), ConfigMutationError> {
    // Keep runtime activation and preference persistence under the same lock:
    // concurrent strategy changes must not let an older request write its
    // preference after a newer request has already become effective.
    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let previous = *state.node_select_preference.read().await;
    let preference_path = &state.runtime_paths.node_select_preference;
    let preference_snapshot = read_file_snapshot(preference_path)
        .await
        .map_err(ConfigMutationError::Apply)?;

    // Persistence is part of accepting the request. Do it before changing the
    // runtime, then restore it if runtime activation fails.
    if let Err(save_err) = save_node_select_preference(state, node_select).await {
        return match restore_file_snapshot(preference_path, preference_snapshot.as_deref()).await {
            Ok(()) => Err(ConfigMutationError::Apply(save_err)),
            Err(rollback_err) => Err(ConfigMutationError::Apply(AppError::message(format!(
                "Failed to persist node-selection strategy: {save_err}. Preference rollback failed: {rollback_err}"
            )))),
        };
    }
    *state.node_select_preference.write().await = node_select;

    let mut new_config = old_config.clone();
    new_config.node_select = node_select;
    let apply_result = if new_config == old_config {
        Ok(RuntimeUpdate::None)
    } else {
        apply_config_change(state, &old_config, &new_config)
            .await
            .map(|effect| effect.runtime_update())
    };
    let update = match apply_result {
        Ok(update) => update,
        Err(apply_err) => {
            *state.node_select_preference.write().await = previous;
            return match restore_file_snapshot(preference_path, preference_snapshot.as_deref()).await
            {
                Ok(()) => Err(ConfigMutationError::Apply(apply_err)),
                Err(rollback_err) => Err(ConfigMutationError::Apply(AppError::message(format!(
                    "Failed to apply node-selection strategy: {apply_err}. Preference rollback failed: {rollback_err}"
                )))),
            };
        }
    };
    let effective = state.config.read().await.node_select;
    Ok((previous, effective, update))
}

/// 最高倍率与节点选择共享同一事务和平台持久化语义。None 表示不限。
pub async fn apply_max_multiplier(
    state: &Arc<AppState>,
    max_multiplier: Option<NodeMultiplier>,
) -> Result<(Option<NodeMultiplier>, RuntimeUpdate), ConfigMutationError> {
    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let previous = *state.max_multiplier_preference.read().await;
    let preference_path = &state.runtime_paths.max_multiplier_preference;
    let preference_snapshot = read_file_snapshot(preference_path)
        .await
        .map_err(ConfigMutationError::Apply)?;

    if let Err(save_err) = save_max_multiplier_preference(state, max_multiplier).await {
        return match restore_file_snapshot(preference_path, preference_snapshot.as_deref()).await {
            Ok(()) => Err(ConfigMutationError::Apply(save_err)),
            Err(rollback_err) => Err(ConfigMutationError::Apply(AppError::message(format!(
                "Failed to persist max-multiplier preference: {save_err}. Preference rollback failed: {rollback_err}"
            )))),
        };
    }
    *state.max_multiplier_preference.write().await = max_multiplier;

    let mut new_config = old_config.clone();
    new_config.max_multiplier = max_multiplier;
    let apply_result = if new_config == old_config {
        Ok(RuntimeUpdate::None)
    } else {
        apply_config_change(state, &old_config, &new_config)
            .await
            .map(|effect| effect.runtime_update())
    };

    match apply_result {
        Ok(update) => Ok((previous, update)),
        Err(apply_err) => {
            *state.max_multiplier_preference.write().await = previous;
            match restore_file_snapshot(preference_path, preference_snapshot.as_deref()).await {
                Ok(()) => Err(ConfigMutationError::Apply(apply_err)),
                Err(rollback_err) => Err(ConfigMutationError::Apply(AppError::message(format!(
                    "Failed to apply max multiplier: {apply_err}. Preference rollback failed: {rollback_err}"
                )))),
            }
        }
    }
}

/// 只用本地材料把磁盘 config.json 恢复到变更前状态：优先内存快照，其次缓存。
/// Ok(true)=已恢复；Ok(false)=本地无材料；Err=有材料但写回失败（磁盘 I/O 故障，
/// 此时重新生成同样会卡在写盘，调用方直接上报即可）。
async fn restore_disk_config(state: &Arc<AppState>, snapshot: Option<&[u8]>) -> AppResult<bool> {
    let mut last_err = None;
    if let Some(bytes) = snapshot {
        match restore_runtime_config_bytes(state, bytes).await {
            Ok(()) => return Ok(true),
            Err(err) => {
                warn!(error = %err, "Failed to restore runtime config from snapshot, trying cache");
                last_err = Some(err);
            }
        }
    }
    if has_config_cache(state) {
        match restore_config_from_cache(state).await {
            Ok(()) => return Ok(true),
            Err(err) => {
                warn!(error = %err, "Failed to restore runtime config from cache");
                last_err = Some(err);
            }
        }
    }
    match last_err {
        Some(err) => Err(err),
        None => Ok(false),
    }
}

async fn restore_previous_running_config(
    old_config: &Config,
    state: &Arc<AppState>,
    snapshot: Option<&[u8]>,
) -> AppResult<()> {
    if is_sing_box_running(state).await {
        // 内核还在跑变更前配置：回滚只是让磁盘重新等于运行中的状态，纯本地操作
        match restore_disk_config(state, snapshot).await {
            Ok(true) => {
                state.runtime_ready.store(true, Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Ready);
                return Ok(());
            }
            Ok(false) => {
                // 本地没有任何材料（新装/清场后）：才退化到重新生成（网络）
                let outcome =
                    regenerate_without_restart_runtime(old_config, state, SubSource::Fetch).await?;
                update_config_warning(old_config, state, outcome.has_sub_nodes).await;
                state.runtime_ready.store(true, Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Ready);
                return Ok(());
            }
            Err(err) => return Err(err),
        }
    }

    restart_with_previous_config(old_config, state, snapshot).await
}

async fn restart_with_previous_config(
    old_config: &Config,
    state: &Arc<AppState>,
    snapshot: Option<&[u8]>,
) -> AppResult<()> {
    #[cfg(not(unix))]
    stop_sing_internal(state).await;

    // 本地材料分层（快照 → 缓存）：写回 → 校验 → 启动，全程不碰网络。
    // 校验挡掉损坏的材料，也兜住内核升级后旧配置不再合法的情况（落到下一层/重新生成）。
    let cache = read_config_cache(state).await;
    for (source, bytes) in [("snapshot", snapshot), ("cache", cache.as_deref())]
        .into_iter()
        .filter_map(|(source, bytes)| bytes.map(|b| (source, b)))
    {
        if let Err(err) = restore_runtime_config_bytes(state, bytes).await {
            warn!(error = %err, source = source, "Failed to write back runtime config, trying next source");
            continue;
        }
        if let Err(err) = validate_sing_box_config(state, &state.runtime_paths.active_config).await
        {
            warn!(error = %err, source = source, "Restored runtime config failed validation, trying next source");
            continue;
        }
        #[cfg(unix)]
        let activation = if is_sing_box_running(state).await {
            crate::services::singbox::reload_sing_internal(state).await
        } else {
            start_sing_internal(state).await
        };
        #[cfg(not(unix))]
        let activation = start_sing_internal(state).await;

        match activation {
            Ok(()) => {
                finalize_started_config(old_config, state, true).await;
                return Ok(());
            }
            Err(err) => {
                warn!(error = %err, source = source, "Failed to activate restored config, trying next source");
            }
        }
    }

    // 本地材料全部不可用/失败，才退化到重新生成（网络）
    // Unix 热重载可能留下一个存活但不健康的进程；同步再启动前统一收口。
    stop_sing_internal(state).await;
    let outcome = regenerate_without_restart_runtime(old_config, state, SubSource::Fetch).await?;
    start_sing_internal(state)
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box with previous config", e))?;
    finalize_started_config(old_config, state, outcome.has_sub_nodes).await;
    Ok(())
}

async fn restore_previous_stopped_config(
    old_config: &Config,
    state: &Arc<AppState>,
    snapshot: Option<&[u8]>,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        clear_runtime_config(state).await;
        state.set_runtime_phase(RuntimePhase::Stopped);
        return Ok(());
    }

    // 服务本就处于停止态：只需把磁盘修回变更前配置，不起进程；
    // 本地无材料才退化到重新生成（网络）
    match restore_disk_config(state, snapshot).await {
        Ok(true) => {
            state.set_runtime_phase(RuntimePhase::Stopped);
            Ok(())
        }
        Ok(false) => {
            let outcome =
                regenerate_without_restart_runtime(old_config, state, SubSource::Fetch).await?;
            update_config_warning(old_config, state, outcome.has_sub_nodes).await;
            state.set_runtime_phase(RuntimePhase::Stopped);
            Ok(())
        }
        Err(err) => Err(err),
    }
}
