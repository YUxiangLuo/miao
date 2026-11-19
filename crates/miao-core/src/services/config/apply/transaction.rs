use super::super::{
    bindings::load_bindings,
    persist::{save_sub_nodes_snapshot, SubNodesSnapshot},
};
use super::*;

/// How runtime rollback should reconcile the sing-box process and disk files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RollbackMode {
    /// Service is intentionally stopped: repair disk artifacts without starting sing-box.
    StoppedDiskOnly,
    /// Service should run, and the candidate was not newly activated: if the
    /// running kernel is still healthy on the previous config, only sync disk
    /// bytes; otherwise validate and restart/reload from local fallback tiers.
    RunningPreserveHealthy,
    /// Service should run, and the candidate was already activated before
    /// commit failed: even a healthy kernel is running uncommitted bytes and
    /// must be reloaded/restarted back onto the previous configuration.
    RunningReactivatePrevious,
}

impl RollbackMode {
    fn for_apply_failure(should_run: bool) -> Self {
        if should_run {
            Self::RunningPreserveHealthy
        } else {
            Self::StoppedDiskOnly
        }
    }

    fn for_commit_failure(should_run: bool, runtime_update: RuntimeUpdate) -> Self {
        if !should_run {
            Self::StoppedDiskOnly
        } else if runtime_update.updated() {
            Self::RunningReactivatePrevious
        } else {
            Self::RunningPreserveHealthy
        }
    }
}

#[derive(Clone, Copy)]
struct RollbackErrorCopy<'a> {
    restored_context: Option<&'static str>,
    failure_prefix: &'a str,
    rollback_prefix: &'static str,
}

impl<'a> RollbackErrorCopy<'a> {
    const fn preserving_cause(failure_prefix: &'a str) -> Self {
        Self {
            restored_context: None,
            failure_prefix,
            rollback_prefix: "Runtime rollback failed",
        }
    }

    const fn with_context(
        restored_context: &'static str,
        failure_prefix: &'a str,
        rollback_prefix: &'static str,
    ) -> Self {
        Self {
            restored_context: Some(restored_context),
            failure_prefix,
            rollback_prefix,
        }
    }
}

const REFRESH_COMMIT_ROLLBACK_COPY: RollbackErrorCopy<'static> = RollbackErrorCopy::with_context(
    "Failed to commit refreshed configuration; restored previous runtime config",
    "Failed to commit refreshed configuration",
    "Runtime rollback failed",
);

const CONFIG_PERSIST_ROLLBACK_COPY: RollbackErrorCopy<'static> = RollbackErrorCopy::with_context(
    "Failed to persist config change; restored previous runtime config",
    "Failed to persist config change",
    "Runtime rollback failed",
);

const EMPTY_POOL_REJECT_ROLLBACK_COPY: RollbackErrorCopy<'static> = RollbackErrorCopy::with_context(
    "配置变更后没有可用节点，已保留当前运行配置；请恢复订阅获取或添加可用手动节点后重试",
    "配置变更后没有可用节点",
    "恢复先前运行状态失败",
);

const CONFIG_APPLY_ROLLBACK_COPY: RollbackErrorCopy<'static> = RollbackErrorCopy::with_context(
    "Failed to apply config change; restored previous runtime config",
    "Failed to apply config change",
    "Runtime rollback failed",
);

/// Rollback material is captured before generation/installation, under the
/// transaction lock. Unreadable files reject the edit rather than losing the
/// ability to restore tag bindings or the active runtime.
struct RuntimeCheckpoint {
    runtime: Option<Vec<u8>>,
    bindings: Option<Vec<u8>>,
}

impl RuntimeCheckpoint {
    async fn capture(state: &AppState) -> AppResult<Self> {
        Ok(Self {
            runtime: read_file_snapshot(&state.runtime_paths.active_config)
                .await?
                .filter(|bytes| !bytes.is_empty()),
            bindings: read_file_snapshot(&state.runtime_paths.node_bindings).await?,
        })
    }

    fn has_any_rollback_material(&self, state: &AppState) -> bool {
        self.runtime.is_some() || has_config_cache(state) || has_sub_nodes_snapshot(state)
    }

    async fn restore(
        &self,
        config: &Config,
        state: &Arc<AppState>,
        mode: RollbackMode,
    ) -> AppResult<()> {
        restore_after_apply_failure(
            config,
            state,
            mode,
            self.runtime.as_deref(),
            self.bindings.as_deref(),
        )
        .await
    }

    async fn restore_and_format_error(
        &self,
        config: &Config,
        state: &Arc<AppState>,
        mode: RollbackMode,
        copy: RollbackErrorCopy<'_>,
        cause: AppError,
    ) -> AppError {
        match self.restore(config, state, mode).await {
            Ok(()) => match copy.restored_context {
                Some(context) => AppError::context(context, cause),
                None => cause,
            },
            Err(rollback_err) => AppError::message(format!(
                "{}: {cause}. {}: {rollback_err}",
                copy.failure_prefix, copy.rollback_prefix
            )),
        }
    }
}

struct PreparedCandidateRuntime {
    outcome: GenConfigOutcome,
    runtime_update: RuntimeUpdate,
}

async fn prepare_candidate_runtime(
    config: &Config,
    state: &Arc<AppState>,
    should_run: bool,
    policy: RefreshPolicy,
    source: SubSource,
) -> AppResult<PreparedCandidateRuntime> {
    if should_run {
        let refresh = regenerate_and_restart_runtime(config, state, policy, source).await?;
        let runtime_update = refresh.runtime_update;
        let outcome = refresh
            .generated
            .ok_or_else(|| AppError::message("Runtime refresh kept the previous configuration"))?;
        Ok(PreparedCandidateRuntime {
            outcome,
            runtime_update,
        })
    } else {
        let outcome = regenerate_without_restart_runtime(config, state, source).await?;
        Ok(PreparedCandidateRuntime {
            outcome,
            runtime_update: RuntimeUpdate::None,
        })
    }
}

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

fn is_subscription_removal(before: &[String], after: &[String]) -> bool {
    after.len() < before.len() && after.iter().all(|url| before.contains(url))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubscriptionMaterialPlan {
    NoOp,
    PreferRetainedSnapshotThenFetch,
    FetchFromNetwork,
}

fn decide_subscription_material_plan(
    force_refresh: bool,
    before_subs: &[String],
    candidate_subs: &[String],
) -> SubscriptionMaterialPlan {
    if !force_refresh && candidate_subs == before_subs {
        SubscriptionMaterialPlan::NoOp
    } else if !force_refresh && is_subscription_removal(before_subs, candidate_subs) {
        SubscriptionMaterialPlan::PreferRetainedSnapshotThenFetch
    } else {
        SubscriptionMaterialPlan::FetchFromNetwork
    }
}

fn rebase_subscription_list(latest_config: &Config, subs: Vec<String>) -> Config {
    let mut rebased = latest_config.clone();
    rebased.subs = subs;
    rebased
        .disabled_nodes
        .retain(|entry| rebased.subs.contains(&entry.sub));
    rebased
}

/// Source removal reuses the retained sources' accepted nodes when possible.
/// Network fetches never own the configuration transaction lock. Only the
/// newest subscription operation may commit; local edits made during the
/// fetch are rebased by copying only the requested subscription list.
pub async fn edit_subscriptions<T>(
    state: &Arc<AppState>,
    force_refresh: bool,
    mutate: impl FnOnce(&mut Vec<String>) -> Result<T, String>,
) -> Result<(T, RuntimeUpdate, crate::models::SubscriptionFetchReport), ConfigMutationError> {
    let guard = state.config_update.lock().await;
    // A queued refresh may acquire this lock after shutdown retired the old
    // generation. It must not mint a fresh generation and escape cancellation.
    if state.lifecycle.is_shutting_down() {
        return Err(ConfigMutationError::Superseded);
    }
    let before = state.config_with_preferences().await;
    let mut candidate = before.clone();
    let value = mutate(&mut candidate.subs).map_err(ConfigMutationError::Rejected)?;
    let plan = decide_subscription_material_plan(force_refresh, &before.subs, &candidate.subs);
    if plan == SubscriptionMaterialPlan::NoOp {
        return Ok((value, RuntimeUpdate::None, Default::default()));
    }
    let generation = state.next_sub_refresh();
    let _foreground = state.subscription_refresh.foreground(generation);
    // Do this before preparing the candidate, including when that edit later
    // fails: the previous fetch has already been cancelled in either case.
    state.settle_cancelled_sub_statuses().await;
    let retained = match plan {
        SubscriptionMaterialPlan::PreferRetainedSnapshotThenFetch => {
            super::super::generate::retained_subscription_nodes(&before, &candidate, state).await
        }
        SubscriptionMaterialPlan::FetchFromNetwork => None,
        SubscriptionMaterialPlan::NoOp => unreachable!("no-op handled above"),
    };
    drop(guard);

    let fetched = match retained {
        Some(nodes) => nodes,
        None => {
            super::super::generate::fetch_sub_nodes_if_current(
                &candidate,
                state,
                SubFetchRetry::None,
                generation,
            )
            .await
        }
    };

    let _guard = state.config_update.lock().await;
    if state.lifecycle.is_shutting_down()
        || state.sub_refresh_generation.load(Ordering::Relaxed) != generation
    {
        return Err(ConfigMutationError::Superseded);
    }
    let old_config = state.config.read().await.clone();
    let new_config = rebase_subscription_list(&old_config, candidate.subs);
    let report = fetched.report;
    let accepted_response = report.accepted_response();
    let update = if new_config.subs == old_config.subs {
        regenerate_from_source(
            &old_config,
            state,
            SubSource::Prefetched(fetched),
            generation,
        )
        .await
    } else {
        apply_config_change_with_source(
            state,
            &old_config,
            &new_config,
            SubSource::Prefetched(fetched),
        )
        .await
        .map(|effect| effect.runtime_update())
    }
    .map_err(ConfigMutationError::Apply)?;
    state
        .sub_status
        .lock()
        .await
        .retain(|url, _| new_config.subs.contains(url));
    if accepted_response {
        state
            .sub_refresh_success_generation
            .store(generation, Ordering::Relaxed);
    }
    Ok((value, update, report))
}

pub async fn refresh_subscriptions_foreground(
    state: &Arc<AppState>,
) -> Result<SubscriptionRefreshOutcome, ConfigMutationError> {
    edit_subscriptions(state, true, |_| Ok(()))
        .await
        .map(|(_, runtime_update, report)| SubscriptionRefreshOutcome {
            fetch_succeeded: report.accepted_response(),
            report,
            runtime_update,
        })
}

#[cfg(test)]
pub async fn regenerate_preserving_service_state(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<RuntimeUpdate> {
    let generation = state.next_sub_refresh();
    let fetched = super::super::generate::fetch_sub_nodes_if_current(
        config,
        state,
        SubFetchRetry::None,
        generation,
    )
    .await;
    regenerate_from_source(config, state, SubSource::Prefetched(fetched), generation).await
}

async fn regenerate_from_source(
    config: &Config,
    state: &Arc<AppState>,
    source: SubSource,
    refresh_generation: u64,
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
    let should_run = state.lifecycle.snapshot().should_run;
    let _activity = state
        .lifecycle
        .activity(crate::state::lifecycle::RuntimeActivity::ApplyingConfig);

    if config_apply_mode(config, should_run) == ConfigApplyMode::Clear {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(RuntimeUpdate::None);
    }

    let checkpoint = RuntimeCheckpoint::capture(state).await?;
    let prepared = match prepare_candidate_runtime(
        config,
        state,
        should_run,
        RefreshPolicy::Manual,
        source,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(err) => {
            // Candidate validation never replaces config.json. Activation
            // may have, so rewind runtime + bindings before surfacing err.
            let failure_prefix = if should_run {
                error!(error = %err, "Failed to refresh subscriptions, restoring previous runtime state");
                "Failed to refresh subscriptions"
            } else {
                error!(error = %err, "Failed to regenerate config, restoring previous runtime config");
                "Failed to regenerate config"
            };
            return Err(checkpoint
                .restore_and_format_error(
                    previous_config,
                    state,
                    RollbackMode::for_apply_failure(should_run),
                    RollbackErrorCopy::preserving_cause(failure_prefix),
                    err,
                )
                .await);
        }
    };

    if let Err(commit_err) = commit_generated(
        config,
        state,
        &prepared.outcome,
        CommitScope::EffectiveSelection,
    )
    .await
    {
        if should_run {
            error!(error = %commit_err, "Foreground runtime refresh could not commit effective preferences; restoring previous runtime state");
        } else {
            error!(error = %commit_err, "Stopped runtime refresh could not commit effective preferences; restoring previous runtime files");
        }
        return Err(checkpoint
            .restore_and_format_error(
                previous_config,
                state,
                RollbackMode::for_commit_failure(should_run, prepared.runtime_update),
                REFRESH_COMMIT_ROLLBACK_COPY,
                commit_err,
            )
            .await);
    }

    publish_runtime_warning_and_proxy(
        config,
        state,
        should_run,
        prepared.runtime_update,
        prepared.outcome.subscription_fetch_failed(),
    )
    .await;

    if prepared.outcome.accepted_subscription_response() {
        state
            .sub_refresh_success_generation
            .store(refresh_generation, Ordering::Relaxed);
    }
    Ok(prepared.runtime_update)
}

pub(in crate::services::config) async fn finalize_started_config(
    config: &Config,
    state: &Arc<AppState>,
    subscription_fetch_failed: bool,
) {
    update_config_warning(config, state, subscription_fetch_failed).await;

    spawn_restore_last_proxy(state);
}

async fn publish_runtime_warning_and_proxy(
    config: &Config,
    state: &Arc<AppState>,
    should_run: bool,
    runtime_update: RuntimeUpdate,
    subscription_fetch_failed: bool,
) {
    if should_run && runtime_update.updated() {
        finalize_started_config(config, state, subscription_fetch_failed).await;
    } else {
        update_config_warning(config, state, subscription_fetch_failed).await;
    }
}

fn classify_apply_effect(should_run: bool, runtime_update: RuntimeUpdate) -> ConfigApplyEffect {
    if !should_run {
        ConfigApplyEffect::Regenerated
    } else if runtime_update.updated() {
        ConfigApplyEffect::Activated(runtime_update)
    } else {
        ConfigApplyEffect::Unchanged
    }
}

enum CommitScope {
    /// Refresh only changes effective selection; retain concurrently edited inputs.
    EffectiveSelection,
    /// Local/subscription edit accepts all candidate inputs and effective selection.
    Configuration,
}

async fn commit_generated(
    config: &Config,
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
    scope: CommitScope,
) -> AppResult<()> {
    // Persistence is the commit point. Do not publish snapshots or diagnostics
    // until it succeeds, otherwise an API error could leave observable state
    // describing runtime bytes that are about to be rolled back.
    match scope {
        CommitScope::EffectiveSelection => {
            persist_effective_node_select(state, outcome.node_select).await?;
        }
        CommitScope::Configuration => {
            let accepted = Config {
                node_select: outcome.node_select,
                ..config.clone()
            };
            save_config_layered(state, &accepted).await?;
            *state.config.write().await = accepted;
        }
    }
    record_fresh_snapshot(config, state, outcome).await;
    publish_generation_diagnostics(state, outcome).await;
    Ok(())
}

async fn update_config_warning(
    config: &Config,
    state: &Arc<AppState>,
    subscription_fetch_failed: bool,
) {
    save_config_cache(state).await;

    let effective = state.config.read().await.node_select;
    *state.config_warning.lock().await = if !config.node_select.is_manual() && effective.is_manual()
    {
        Some(REGION_FALLBACK.to_string())
    } else if subscription_fetch_failed && !config.subs.is_empty() {
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
        SubSource::SnapshotOrLocal => gen_config_from_snapshot(config, state).await,
        SubSource::Prefetched(nodes) => gen_config_from_fetch(config, state, nodes).await,
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
    *state.sub_nodes_cache.write().await = None;
    state.data_revision.fetch_add(1, Ordering::Relaxed);
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

async fn publish_empty_pool_state(state: &AppState, persisted_config: Config) {
    let warning = no_usable_nodes_warning(&persisted_config);
    *state.config.write().await = persisted_config;
    *state.config_warning.lock().await = Some(warning);
    state.skipped_rules.lock().await.clear();
    state.available_multipliers.write().await.clear();
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
    *state.sub_nodes_cache.write().await = None;
    state.data_revision.fetch_add(1, Ordering::Relaxed);
    publish_empty_pool_state(state, persisted_config).await;
    Ok(())
}

async fn persist_config_without_usable_nodes(
    state: &Arc<AppState>,
    persisted_config: Config,
    snapshot: Option<SubNodesSnapshot>,
) -> AppResult<()> {
    let Some(snapshot) = snapshot else {
        return persist_config_without_usable_nodes_at(
            state,
            persisted_config,
            &state.runtime_paths.active_config,
            &state.runtime_paths.config_cache,
            &state.runtime_paths.sub_nodes_snapshot,
        )
        .await;
    };

    // An empty runtime can still have disabled canonical nodes. Preserve that
    // material and bind it to this commit, just like a generated runtime does.
    // Otherwise re-enabling a node would require another subscription fetch.
    let old_bindings = read_file_snapshot(&state.runtime_paths.node_bindings).await?;
    let mut bindings = load_bindings(state).await?.unwrap_or_default();
    bindings.subscription_snapshot_sha256 = Some(snapshot.fingerprint()?);
    // A successful deletion must never leave old bytes at an active/cache
    // path. Stage their removal before committing; keep the old process alive
    // and restore the files if either retirement or persistence fails.
    let retired = super::retirement::RuntimeRetirement::stage(&state.runtime_paths).await?;
    let commit = async {
        save_node_bindings(state, &bindings).await?;
        save_config_layered(state, &persisted_config).await
    }
    .await;
    if let Err(err) = commit {
        // Neither the running process nor the accepted snapshot has changed.
        let bindings_restore =
            restore_file_snapshot(&state.runtime_paths.node_bindings, old_bindings.as_deref())
                .await;
        let files_restore = retired.restore().await;
        let errors: Vec<_> = [bindings_restore, files_restore]
            .into_iter()
            .filter_map(Result::err)
            .map(|err| err.to_string())
            .collect();
        return if errors.is_empty() {
            Err(err)
        } else {
            Err(AppError::message(format!(
                "Failed to persist configuration without usable nodes: {err}. Rollback failed: {}",
                errors.join("; ")
            )))
        };
    }

    stop_sing_internal(state).await;
    retired.discard().await;
    // Publishes to memory even if the derived disk write fails. The committed
    // fingerprint then prevents an older snapshot being reused after restart.
    if let Err(err) = save_sub_nodes_snapshot(state, &snapshot).await {
        warn!(error = %err, "Failed to save subscription snapshot without usable nodes");
    }
    publish_empty_pool_state(state, persisted_config).await;
    Ok(())
}

async fn restore_previous_config(
    old_config: &Config,
    state: &Arc<AppState>,
    mode: RollbackMode,
    snapshot: Option<&[u8]>,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(());
    }

    match mode {
        RollbackMode::RunningReactivatePrevious => {
            restart_with_previous_config(old_config, state, snapshot).await
        }
        RollbackMode::RunningPreserveHealthy => {
            restore_previous_running_config(old_config, state, snapshot).await
        }
        RollbackMode::StoppedDiskOnly => {
            restore_previous_stopped_config(old_config, state, snapshot).await
        }
    }
}

async fn restore_after_apply_failure(
    old_config: &Config,
    state: &Arc<AppState>,
    mode: RollbackMode,
    runtime_snapshot: Option<&[u8]>,
    bindings_snapshot: Option<&[u8]>,
) -> AppResult<()> {
    // Bindings commit together with config.json. Restore them even if runtime
    // recovery failed so the next successful transaction starts from old state.
    let runtime_restore = restore_previous_config(old_config, state, mode, runtime_snapshot).await;
    let bindings_restore =
        restore_file_snapshot(&state.runtime_paths.node_bindings, bindings_snapshot).await;

    combine_rollback_results(runtime_restore, bindings_restore)
}

// Low-level fault-injection tests supply their own before/candidate snapshots.
// Production local edits must own ConfigEdit; subscriptions must prefetch.
#[cfg(test)]
pub async fn apply_config_change(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
) -> AppResult<ConfigApplyEffect> {
    let source = if old_config.subs == new_config.subs {
        SubSource::SnapshotOrLocal
    } else {
        let generation = state.next_sub_refresh();
        SubSource::Prefetched(
            super::super::generate::fetch_sub_nodes_if_current(
                new_config,
                state,
                SubFetchRetry::None,
                generation,
            )
            .await,
        )
    };
    apply_config_change_with_source(state, old_config, new_config, source).await
}

#[derive(Clone, Debug)]
enum EmptyNodePoolDecision {
    /// Fetch failed (or concurrent edit emptied a cached pool) while previous
    /// local material exists: reject the change and restore previous state.
    KeepPreviousRuntime,
    /// Authoritative subscription removal, or initial setup without rollback
    /// material: persist the empty-pool config, stop sing-box, and retain
    /// remaining canonical nodes (including disabled ones) when present.
    AcceptWithoutRuntime {
        retained_snapshot: Option<SubNodesSnapshot>,
    },
}

/// A successful empty response (or deleting the final subscription) is
/// authoritative: a removal must not be undone just because it leaves no usable
/// nodes. By contrast, a total fetch failure or unverified local snapshot keeps
/// `None` so existing runtime material can protect the data plane.
fn authoritative_removal_snapshot(
    old_subs: &[String],
    new_subs: &[String],
    source: &SubSource,
) -> Option<SubNodesSnapshot> {
    match source {
        SubSource::Prefetched(fetched)
            if is_subscription_removal(old_subs, new_subs)
                && (new_subs.is_empty() || fetched.report.accepted_response()) =>
        {
            Some(SubNodesSnapshot::from_fetched_nodes(
                new_subs.to_vec(),
                fetched.nodes.clone(),
            ))
        }
        _ => None,
    }
}

fn decide_empty_node_pool(
    removal_snapshot: Option<SubNodesSnapshot>,
    has_rollback_material: bool,
) -> EmptyNodePoolDecision {
    if removal_snapshot.is_some() || !has_rollback_material {
        EmptyNodePoolDecision::AcceptWithoutRuntime {
            retained_snapshot: removal_snapshot,
        }
    } else {
        EmptyNodePoolDecision::KeepPreviousRuntime
    }
}

pub(super) async fn apply_config_change_with_source(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
    source: SubSource,
) -> AppResult<ConfigApplyEffect> {
    // Application edits clone the effective config, whose strategy may be a temporary
    // manual fallback. Configuration changes must regenerate with the user's
    // requested strategy instead of accidentally extending that fallback.
    let preferred_new_config = state.overlay_preferences(new_config).await;
    let new_config = &preferred_new_config;
    let should_run = state.lifecycle.snapshot().should_run;
    let apply_mode = config_apply_mode(new_config, should_run);

    if apply_mode == ConfigApplyMode::Clear {
        let _activity = state
            .lifecycle
            .activity(crate::state::lifecycle::RuntimeActivity::ApplyingConfig);
        save_config_layered(state, new_config).await?;
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        *state.config.write().await = new_config.clone();
        *state.skipped_rules.lock().await = Vec::new();
        *state.available_multipliers.write().await = Vec::new();
        return Ok(ConfigApplyEffect::Cleared);
    }

    let removal_snapshot =
        authoritative_removal_snapshot(&old_config.subs, &new_config.subs, &source);
    let checkpoint = RuntimeCheckpoint::capture(state).await?;
    let _activity = state
        .lifecycle
        .activity(crate::state::lifecycle::RuntimeActivity::ApplyingConfig);

    let should_activate = match apply_mode {
        ConfigApplyMode::Restart => true,
        ConfigApplyMode::RegenerateOnly => false,
        ConfigApplyMode::Clear => unreachable!("clear mode handled above"),
    };

    match prepare_candidate_runtime(
        new_config,
        state,
        should_activate,
        RefreshPolicy::ManualInApply,
        source,
    )
    .await
    {
        Ok(prepared) => {
            if let Err(save_err) = commit_generated(
                new_config,
                state,
                &prepared.outcome,
                CommitScope::Configuration,
            )
            .await
            {
                error!(error = %save_err, "Runtime config applied but persistent config write failed, attempting runtime rollback");
                return Err(checkpoint
                    .restore_and_format_error(
                        old_config,
                        state,
                        RollbackMode::for_commit_failure(should_run, prepared.runtime_update),
                        CONFIG_PERSIST_ROLLBACK_COPY,
                        save_err,
                    )
                    .await);
            }

            publish_runtime_warning_and_proxy(
                new_config,
                state,
                should_run,
                prepared.runtime_update,
                prepared.outcome.subscription_fetch_failed(),
            )
            .await;
            Ok(classify_apply_effect(should_run, prepared.runtime_update))
        }
        Err(apply_err) if apply_err.is_no_usable_nodes() => {
            match decide_empty_node_pool(
                removal_snapshot,
                checkpoint.has_any_rollback_material(state),
            ) {
                EmptyNodePoolDecision::KeepPreviousRuntime => {
                    warn!(error = %apply_err, "Config change left no usable nodes; keeping previous runtime state");
                    Err(checkpoint
                        .restore_and_format_error(
                            old_config,
                            state,
                            RollbackMode::for_apply_failure(should_run),
                            EMPTY_POOL_REJECT_ROLLBACK_COPY,
                            apply_err,
                        )
                        .await)
                }
                EmptyNodePoolDecision::AcceptWithoutRuntime { retained_snapshot } => {
                    warn!(error = %apply_err, "Config change left no usable nodes; persisting it and stopping sing-box");
                    persist_config_without_usable_nodes(
                        state,
                        new_config.clone(),
                        retained_snapshot,
                    )
                    .await
                    .map(|()| ConfigApplyEffect::Cleared)
                }
            }
        }
        Err(apply_err) => {
            error!(error = %apply_err, "Failed to apply runtime config change, attempting runtime rollback");
            Err(checkpoint
                .restore_and_format_error(
                    old_config,
                    state,
                    RollbackMode::for_apply_failure(should_run),
                    CONFIG_APPLY_ROLLBACK_COPY,
                    apply_err,
                )
                .await)
        }
    }
}

/// A rejected candidate, an obsolete subscription operation, or a failed apply.
/// Transport adapters decide how to represent each outcome.
#[derive(Debug)]
pub enum ConfigMutationError {
    Rejected(String),
    Superseded,
    Apply(AppError),
}

impl std::fmt::Display for ConfigMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(message) => write!(f, "{message}"),
            Self::Superseded => write!(f, "订阅操作已被更新的请求或停止服务取代"),
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
    let mut edit = ConfigEdit::begin(state).await;
    edit.candidate = state.overlay_preferences(edit.original()).await;
    mutate(&mut edit.candidate).map_err(ConfigMutationError::Rejected)?;
    if edit.candidate == *edit.original() {
        return Ok(RuntimeUpdate::None);
    }
    edit.commit()
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiskRestoreOutcome {
    Restored,
    MissingMaterial,
}

/// 只用本地材料把磁盘 config.json 恢复到变更前状态：优先内存快照，其次缓存。
/// Restored=已恢复；MissingMaterial=本地无材料；Err=有材料但写回失败（磁盘 I/O 故障，
/// 此时重新生成同样会卡在写盘，调用方直接上报即可）。
async fn restore_disk_config(
    state: &Arc<AppState>,
    snapshot: Option<&[u8]>,
) -> AppResult<DiskRestoreOutcome> {
    let mut last_err = None;
    if let Some(bytes) = snapshot {
        match restore_runtime_config_bytes(state, bytes).await {
            Ok(()) => return Ok(DiskRestoreOutcome::Restored),
            Err(err) => {
                warn!(error = %err, "Failed to restore runtime config from snapshot, trying cache");
                last_err = Some(err);
            }
        }
    }
    if has_config_cache(state) {
        match restore_config_from_cache(state).await {
            Ok(()) => return Ok(DiskRestoreOutcome::Restored),
            Err(err) => {
                warn!(error = %err, "Failed to restore runtime config from cache");
                last_err = Some(err);
            }
        }
    }
    match last_err {
        Some(err) => Err(err),
        None => Ok(DiskRestoreOutcome::MissingMaterial),
    }
}

async fn restore_previous_running_config(
    old_config: &Config,
    state: &Arc<AppState>,
    snapshot: Option<&[u8]>,
) -> AppResult<()> {
    if crate::services::singbox::kernel_status(state).await.ready {
        // 内核还在跑变更前配置：回滚只是让磁盘重新等于运行中的状态，纯本地操作
        return match restore_disk_config(state, snapshot).await? {
            DiskRestoreOutcome::Restored => Ok(()),
            DiskRestoreOutcome::MissingMaterial => Err(AppError::message(
                "运行配置回滚缺少本地快照，请刷新订阅恢复",
            )),
        };
    }

    restart_with_previous_config(old_config, state, snapshot).await
}

async fn restart_with_previous_config(
    old_config: &Config,
    state: &Arc<AppState>,
    snapshot: Option<&[u8]>,
) -> AppResult<()> {
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

        let activation = if is_sing_box_running(state).await {
            crate::services::singbox::reload_sing_internal(state).await
        } else {
            start_sing_internal(state).await
        };

        match activation {
            Ok(()) => {
                finalize_started_config(old_config, state, false).await;
                return Ok(());
            }
            Err(err) => {
                warn!(error = %err, source = source, "Failed to activate restored config, trying next source");
            }
        }
    }

    // 本地材料不可用时只尝试节点快照/手动节点；网络恢复由锁外刷新负责。
    // Unix 热重载可能留下一个存活但不健康的进程；同步再启动前统一收口。
    stop_sing_internal(state).await;
    let outcome =
        regenerate_without_restart_runtime(old_config, state, SubSource::SnapshotOrLocal).await?;
    start_sing_internal(state)
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box with previous config", e))?;
    finalize_started_config(old_config, state, outcome.subscription_fetch_failed()).await;
    Ok(())
}

async fn restore_previous_stopped_config(
    old_config: &Config,
    state: &Arc<AppState>,
    snapshot: Option<&[u8]>,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        clear_runtime_config(state).await;
        return Ok(());
    }

    // 服务本就处于停止态：只需把磁盘修回变更前配置，不起进程；
    // 本地无快照/缓存才退化到本地订阅快照/手动节点重建（绝不持锁触网）。
    match restore_disk_config(state, snapshot).await? {
        DiskRestoreOutcome::Restored => Ok(()),
        DiskRestoreOutcome::MissingMaterial => {
            let outcome =
                regenerate_without_restart_runtime(old_config, state, SubSource::SnapshotOrLocal)
                    .await?;
            update_config_warning(old_config, state, outcome.subscription_fetch_failed()).await;
            Ok(())
        }
    }
}

#[cfg(test)]
mod decision_tests {
    use super::*;
    use crate::models::{DisabledNode, SubscriptionFetchReport};

    #[test]
    fn rollback_mode_distinguishes_apply_and_commit_failures() {
        assert_eq!(
            RollbackMode::for_apply_failure(false),
            RollbackMode::StoppedDiskOnly
        );
        assert_eq!(
            RollbackMode::for_apply_failure(true),
            RollbackMode::RunningPreserveHealthy
        );
        assert_eq!(
            RollbackMode::for_commit_failure(false, RuntimeUpdate::Reloaded),
            RollbackMode::StoppedDiskOnly
        );
        assert_eq!(
            RollbackMode::for_commit_failure(true, RuntimeUpdate::None),
            RollbackMode::RunningPreserveHealthy
        );
        for update in [
            RuntimeUpdate::Started,
            RuntimeUpdate::Reloaded,
            RuntimeUpdate::Restarted,
        ] {
            assert_eq!(
                RollbackMode::for_commit_failure(true, update),
                RollbackMode::RunningReactivatePrevious
            );
        }
    }

    #[test]
    fn subscription_material_plan_reuses_snapshot_only_for_unforced_strict_removal() {
        let a = "https://a.example/sub".to_string();
        let b = "https://b.example/sub".to_string();
        let c = "https://c.example/sub".to_string();
        let ab = [a.clone(), b.clone()];
        let ac = [a.clone(), c];

        assert_eq!(
            decide_subscription_material_plan(
                false,
                std::slice::from_ref(&a),
                std::slice::from_ref(&a)
            ),
            SubscriptionMaterialPlan::NoOp
        );
        assert_eq!(
            decide_subscription_material_plan(
                true,
                std::slice::from_ref(&a),
                std::slice::from_ref(&a)
            ),
            SubscriptionMaterialPlan::FetchFromNetwork
        );
        assert_eq!(
            decide_subscription_material_plan(false, &ab, std::slice::from_ref(&b)),
            SubscriptionMaterialPlan::PreferRetainedSnapshotThenFetch
        );
        assert_eq!(
            decide_subscription_material_plan(true, &ab, std::slice::from_ref(&b)),
            SubscriptionMaterialPlan::FetchFromNetwork
        );
        assert_eq!(
            decide_subscription_material_plan(false, &ab, &ac),
            SubscriptionMaterialPlan::FetchFromNetwork
        );
    }

    #[test]
    fn empty_pool_decision_accepts_only_authoritative_removal_or_cold_setup() {
        let a = "https://a.example/sub".to_string();
        let b = "https://b.example/sub".to_string();
        let ab = [a.clone(), b.clone()];

        // Snapshot-derived NotRequested material (successful_sources == 0) is NOT
        // an authoritative empty HTTP response if a concurrent edit disabled all nodes.
        let unverified_cached = SubSource::Prefetched(FetchedSubscriptions {
            nodes: Vec::new(),
            report: SubscriptionFetchReport {
                cached_nodes: 1,
                ..Default::default()
            },
        });
        assert!(
            authoritative_removal_snapshot(&ab, std::slice::from_ref(&b), &unverified_cached)
                .is_none()
        );
        assert!(matches!(
            decide_empty_node_pool(None, true),
            EmptyNodePoolDecision::KeepPreviousRuntime
        ));

        // Total fetch failure also keeps previous runtime when rollback material exists.
        let failed_fetch = SubSource::Prefetched(FetchedSubscriptions {
            nodes: Vec::new(),
            report: SubscriptionFetchReport {
                failed_sources: 1,
                ..Default::default()
            },
        });
        assert!(
            authoritative_removal_snapshot(&ab, std::slice::from_ref(&b), &failed_fetch).is_none()
        );

        // Fresh accepted empty response (even partial failure) IS authoritative for removal.
        let accepted_empty = SubSource::Prefetched(FetchedSubscriptions {
            nodes: Vec::new(),
            report: SubscriptionFetchReport {
                successful_sources: 1,
                failed_sources: 1,
                ..Default::default()
            },
        });
        let snapshot =
            authoritative_removal_snapshot(&ab, std::slice::from_ref(&b), &accepted_empty);
        assert!(snapshot.is_some());
        assert!(matches!(
            decide_empty_node_pool(snapshot, true),
            EmptyNodePoolDecision::AcceptWithoutRuntime {
                retained_snapshot: Some(_)
            }
        ));

        // Deleting the last subscription is authoritative without network responses.
        let last_removed = SubSource::Prefetched(FetchedSubscriptions::default());
        assert!(
            authoritative_removal_snapshot(std::slice::from_ref(&a), &[], &last_removed).is_some()
        );

        // Cold initial setup with no rollback material accepts empty pool without snapshot.
        assert!(matches!(
            decide_empty_node_pool(None, false),
            EmptyNodePoolDecision::AcceptWithoutRuntime {
                retained_snapshot: None
            }
        ));
    }

    #[test]
    fn rebase_subscription_list_preserves_concurrent_local_fields_and_prunes_disabled() {
        let concurrent = Config {
            subs: vec!["https://a".into(), "https://b".into()],
            route_mode: RouteMode::Global,
            disabled_nodes: vec![
                DisabledNode {
                    sub: "https://a".into(),
                    name: "gone".into(),
                },
                DisabledNode {
                    sub: "https://b".into(),
                    name: "kept".into(),
                },
            ],
            ..Config::default()
        };
        let rebased = rebase_subscription_list(&concurrent, vec!["https://b".into()]);
        assert_eq!(rebased.route_mode, RouteMode::Global);
        assert_eq!(rebased.subs, ["https://b"]);
        assert_eq!(rebased.disabled_nodes.len(), 1);
        assert_eq!(rebased.disabled_nodes[0].sub, "https://b");
    }

    #[test]
    fn classify_apply_effect_maps_stopped_unchanged_and_activated() {
        assert_eq!(
            classify_apply_effect(false, RuntimeUpdate::None),
            ConfigApplyEffect::Regenerated
        );
        assert_eq!(
            classify_apply_effect(true, RuntimeUpdate::None),
            ConfigApplyEffect::Unchanged
        );
        assert_eq!(
            classify_apply_effect(true, RuntimeUpdate::Reloaded),
            ConfigApplyEffect::Activated(RuntimeUpdate::Reloaded)
        );
    }
}
