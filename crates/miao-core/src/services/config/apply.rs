use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, NodeSelect, RouteMode, RuntimePhase};
use crate::services::{
    proxy::spawn_restore_last_proxy,
    singbox::{
        is_sing_box_running, start_sing_internal, stop_sing_internal, validate_sing_box_config,
    },
};
use crate::state::AppState;

use super::bindings::save_node_bindings;
use super::generate::{
    gen_config, gen_config_from_nodes, gen_config_from_snapshot, record_fresh_snapshot,
    FetchedNode, GenConfigOutcome, SubFetchRetry,
};
use super::persist::{
    has_config_cache, has_sub_nodes_snapshot, persist_effective_node_select, read_config_cache,
    restore_config_from_cache, restore_runtime_config_bytes, save_config_cache,
    save_config_layered, snapshot_runtime_config, write_file_atomic,
};
use super::warnings::{ALL_SUBS_FAILED, NO_USABLE_MANUAL, NO_USABLE_SUBS, REGION_FALLBACK};

/// 订阅刷新策略：机制（拉取 → 生成 → 校验 → 激活）只有一条，差异显式表达
pub enum RefreshPolicy {
    /// 手动刷新（面板「刷新订阅」等独立路径）：生成+校验成功后，运行字节
    /// 有变化才激活（Unix reload / 未在跑则 start）；全部订阅失败也用现有
    /// 结果继续；激活后由本管线持久化生效的 node_select
    Manual,
    /// apply_config_change 事务内的刷新：机制同 Manual，但 node_select 由外层
    /// 事务随新配置一并提交，本管线不提前写盘——避免「旧配置 + 新选择」的中间快照
    ManualInApply,
    /// 启动路径：与启动所用缓存比对无变化则不激活。
    /// 内核已在跑时，全部订阅失败/校验失败保留当前运行配置。
    /// 内核未在跑时（恢复路径）不把失败当成「保持运行」：有可用生成
    /// 结果则激活，否则由调用方回退到兼容缓存启动。
    Startup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshEffect {
    Activated,
    SkippedUnchanged,
    KeptRunningOnTotalFailure,
    KeptRunningOnValidationFailure,
}

/// How an accepted runtime update changed the managed process. Keeping this
/// separate from "bytes changed" lets API/MCP callers report the real impact
/// instead of inferring it from the operating system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RuntimeUpdate {
    #[default]
    None,
    Started,
    Reloaded,
    Restarted,
}

impl RuntimeUpdate {
    pub fn updated(self) -> bool {
        self != Self::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfigApplyEffect {
    /// Different runtime bytes were activated by starting, reloading or
    /// replacing the managed process.
    Activated(RuntimeUpdate),
    /// Runtime bytes were prepared while the service was intentionally stopped.
    Regenerated,
    /// The persisted input changed, but generated runtime bytes were identical.
    Unchanged,
    /// No usable source remains, so runtime artifacts were cleared.
    Cleared,
}

impl ConfigApplyEffect {
    pub(crate) fn runtime_update(self) -> RuntimeUpdate {
        match self {
            Self::Activated(update) => update,
            Self::Regenerated | Self::Unchanged | Self::Cleared => RuntimeUpdate::None,
        }
    }
}

pub struct RefreshOutcome {
    pub effect: RefreshEffect,
    pub runtime_update: RuntimeUpdate,
    pub node_select: NodeSelect,
    pub generated: Option<GenConfigOutcome>,
}

/// 生成配置时订阅节点集的来源：真拉取，或优先用上次拉取的快照零网络重建。
#[derive(Clone, Debug, PartialEq)]
pub enum SubSource {
    /// 真拉取（增删订阅/手动刷新/启动）
    Fetch,
    /// 快照优先，缺失或与当前订阅列表不匹配时退化到真拉取
    /// （本地语义变更：节点选择/路由模式/规则/手动节点——切换不是刷新）
    SnapshotOrFetch,
    /// 已预拉取的订阅节点集（启动后台刷新：网络等待在配置锁外完成，
    /// 持锁落地阶段只复用结果，不再碰网络）
    Prefetched(Vec<FetchedNode>),
}

/// 订阅列表没变就是本地语义变更，走快照重建；变了才需要真拉取
pub(super) fn sub_source_for(old_config: &Config, new_config: &Config) -> SubSource {
    if old_config.subs == new_config.subs {
        SubSource::SnapshotOrFetch
    } else {
        SubSource::Fetch
    }
}

/// 刷新后是否需要激活新配置：与当前运行字节不同才需要；读不出内容时保守激活
pub(super) fn config_changed_after_refresh(cache: Option<&[u8]>, current: Option<&[u8]>) -> bool {
    match (cache, current) {
        (Some(old), Some(new)) => old != new,
        _ => true,
    }
}

fn next_candidate_path(state: &AppState) -> std::path::PathBuf {
    static CANDIDATE_ID: AtomicU64 = AtomicU64::new(0);
    let id = CANDIDATE_ID.fetch_add(1, Ordering::Relaxed);
    state
        .runtime_paths
        .runtime_dir
        .join(format!("config.json.next.{}.{}", std::process::id(), id))
}

async fn validate_prepared_config(state: &Arc<AppState>, bytes: &[u8]) -> AppResult<()> {
    let candidate = next_candidate_path(state);
    write_file_atomic(&candidate, bytes).await?;
    let result = validate_sing_box_config(state, &candidate).await;
    if let Err(err) = tokio::fs::remove_file(&candidate).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(path = ?candidate, error = %err, "Failed to remove validated candidate config");
        }
    }
    result
}

async fn restore_runtime_files(
    state: &AppState,
    old_runtime: Option<&[u8]>,
    old_bindings: Option<&[u8]>,
) -> AppResult<()> {
    let runtime_rollback =
        restore_file_snapshot(&state.runtime_paths.active_config, old_runtime).await;
    let bindings_rollback =
        restore_file_snapshot(&state.runtime_paths.node_bindings, old_bindings).await;
    runtime_rollback.and(bindings_rollback)
}

async fn commit_runtime_files(
    state: &AppState,
    bytes: &[u8],
    bindings: &super::bindings::NodeTagBindings,
    old_runtime: Option<&[u8]>,
    old_bindings: Option<&[u8]>,
) -> AppResult<()> {
    let commit = async {
        write_file_atomic(&state.runtime_paths.active_config, bytes).await?;
        save_node_bindings(state, bindings).await
    }
    .await;
    if let Err(commit_err) = commit {
        if let Err(rollback_err) = restore_runtime_files(state, old_runtime, old_bindings).await {
            return Err(AppError::message(format!(
                "{}. Runtime file rollback failed: {}",
                commit_err, rollback_err
            )));
        }
        return Err(commit_err);
    }
    Ok(())
}

/// Validate and install a generated config for startup before any process is
/// launched. The caller publishes cache/snapshot state only after start.
pub async fn install_prepared_runtime(
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
) -> AppResult<()> {
    validate_prepared_config(state, &outcome.bytes).await?;
    let old_runtime = read_file_snapshot(&state.runtime_paths.active_config).await?;
    let old_bindings = read_file_snapshot(&state.runtime_paths.node_bindings).await?;
    commit_runtime_files(
        state,
        &outcome.bytes,
        &outcome.node_bindings,
        old_runtime.as_deref(),
        old_bindings.as_deref(),
    )
    .await
}

async fn restore_file_snapshot(path: &Path, bytes: Option<&[u8]>) -> AppResult<()> {
    match bytes {
        Some(bytes) => write_file_atomic(path, bytes).await,
        None => match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(AppError::context("Failed to remove transaction file", err)),
        },
    }
}

async fn read_file_snapshot(path: &Path) -> AppResult<Option<Vec<u8>>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(AppError::context(
            format!("Failed to snapshot transaction file {}", path.display()),
            err,
        )),
    }
}

#[cfg(not(unix))]
async fn restore_after_activation_failure(
    state: &Arc<AppState>,
    old_runtime: Option<&[u8]>,
    old_bindings: Option<&[u8]>,
) -> AppResult<()> {
    stop_sing_internal(state).await;
    restore_file_snapshot(&state.runtime_paths.node_bindings, old_bindings).await?;
    match old_runtime {
        Some(bytes) => {
            write_file_atomic(&state.runtime_paths.active_config, bytes).await?;
            start_sing_internal(state)
                .await
                .map_err(|e| AppError::context("Failed to restart previous sing-box config", e))
        }
        None => restore_file_snapshot(&state.runtime_paths.active_config, None).await,
    }
}

async fn activate_running_config(
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
) -> AppResult<RuntimeUpdate> {
    let old_runtime = snapshot_runtime_config(state).await;
    let old_bindings = read_file_snapshot(&state.runtime_paths.node_bindings).await?;
    let was_running = is_sing_box_running(state).await;

    #[cfg(unix)]
    {
        use crate::services::singbox::reload_sing_internal;

        // Commit both files before asking sing-box to reload. If either write
        // fails, the running process is still untouched and rollback is purely
        // local. Once SIGHUP is sent, any failure restores the old files and
        // reactivates them before returning.
        commit_runtime_files(
            state,
            &outcome.bytes,
            &outcome.node_bindings,
            old_runtime.as_deref(),
            old_bindings.as_deref(),
        )
        .await?;

        let (activation, runtime_update) = if was_running {
            (reload_sing_internal(state).await, RuntimeUpdate::Reloaded)
        } else {
            (start_sing_internal(state).await, RuntimeUpdate::Started)
        };
        if let Err(reload_err) = activation {
            if let Err(restore_err) =
                restore_runtime_files(state, old_runtime.as_deref(), old_bindings.as_deref()).await
            {
                return Err(AppError::message(format!(
                    "{}. Runtime file rollback failed: {}",
                    reload_err, restore_err
                )));
            }

            // A failed reload may still be inside sing-box's close/recreate
            // loop. Do not queue another SIGHUP into that ambiguous state;
            // failure recovery may interrupt once, then starts the known-good
            // previous bytes deterministically.
            stop_sing_internal(state).await;
            let reactivate = start_sing_internal(state).await;
            return match reactivate {
                Ok(()) => Err(reload_err),
                Err(rollback_err) => Err(AppError::message(format!(
                    "{}. Runtime rollback failed: {}",
                    reload_err, rollback_err
                ))),
            };
        }

        Ok(runtime_update)
    }

    #[cfg(not(unix))]
    {
        stop_sing_internal(state).await;

        let activate_result = async {
            write_file_atomic(&state.runtime_paths.active_config, &outcome.bytes).await?;
            start_sing_internal(state)
                .await
                .map_err(|e| AppError::context("Failed to restart sing-box", e))?;
            save_node_bindings(state, &outcome.node_bindings).await
        }
        .await;

        if let Err(activate_err) = activate_result {
            return match restore_after_activation_failure(
                state,
                old_runtime.as_deref(),
                old_bindings.as_deref(),
            )
            .await
            {
                Ok(()) => Err(activate_err),
                Err(rollback_err) => Err(AppError::message(format!(
                    "{}. Runtime rollback failed: {}",
                    activate_err, rollback_err
                ))),
            };
        }
        Ok(if was_running {
            RuntimeUpdate::Restarted
        } else {
            RuntimeUpdate::Started
        })
    }
}

/// 订阅刷新统一管线：获取节点集（source 决定真拉取还是快照重建）→ 生成配置 →（策略门控）→ 校验 → 激活内核。
/// 不负责缓存保存和告警文案（ManualInApply 也不提前写 node_select）。
pub async fn refresh_subscriptions(
    config: &Config,
    state: &Arc<AppState>,
    policy: RefreshPolicy,
    source: SubSource,
) -> AppResult<RefreshOutcome> {
    let startup = matches!(policy, RefreshPolicy::Startup);
    // 启动路径的订阅全失败多为「先于路由/DHCP 就绪」的瞬态，给退避预算；
    // 手动刷新用户在场，失败即报，不重试
    let retry = if startup {
        SubFetchRetry::Startup
    } else {
        SubFetchRetry::None
    };

    let active_bytes = snapshot_runtime_config(state).await;
    let generated = match source {
        SubSource::Fetch => gen_config(config, state, retry).await,
        SubSource::SnapshotOrFetch => gen_config_from_snapshot(config, state).await,
        SubSource::Prefetched(nodes) => gen_config_from_nodes(config, state, nodes).await,
    }
    .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    let runtime_ready =
        state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await;
    if startup && !generated.has_sub_nodes && !config.subs.is_empty() {
        if runtime_ready {
            return Ok(RefreshOutcome {
                effect: RefreshEffect::KeptRunningOnTotalFailure,
                runtime_update: RuntimeUpdate::None,
                node_select: generated.node_select,
                generated: None,
            });
        }
        info!(
            "Startup fetch produced no subscription nodes; activating generated config because the data plane is not running"
        );
    }

    if runtime_ready
        && !config_changed_after_refresh(active_bytes.as_deref(), Some(&generated.bytes))
    {
        info!("Generated runtime config is unchanged; sing-box keeps running");
        save_node_bindings(state, &generated.node_bindings).await?;
        // ManualInApply belongs to the outer configuration transaction. Do not
        // publish diagnostics or effective preferences before that transaction
        // has durably committed.
        if !matches!(policy, RefreshPolicy::ManualInApply) {
            record_fresh_snapshot(config, state, &generated).await;
            persist_effective_node_select(state, generated.node_select).await?;
            *state.skipped_rules.lock().await = generated.skipped_rules.clone();
        }
        return Ok(RefreshOutcome {
            effect: RefreshEffect::SkippedUnchanged,
            runtime_update: RuntimeUpdate::None,
            node_select: generated.node_select,
            generated: Some(generated),
        });
    }

    if let Err(e) = validate_prepared_config(state, &generated.bytes).await {
        if startup && runtime_ready {
            return Ok(RefreshOutcome {
                effect: RefreshEffect::KeptRunningOnValidationFailure,
                runtime_update: RuntimeUpdate::None,
                node_select: generated.node_select,
                generated: None,
            });
        }
        return Err(AppError::context(
            "Config validation failed, not restarting",
            e,
        ));
    }
    let runtime_update = activate_running_config(state, &generated).await?;
    info!(
        ?runtime_update,
        "sing-box runtime config activated successfully"
    );
    // ManualInApply 的 node_select 由外层 apply_config_change 事务一并提交
    if !matches!(policy, RefreshPolicy::ManualInApply) {
        record_fresh_snapshot(config, state, &generated).await;
        *state.skipped_rules.lock().await = generated.skipped_rules.clone();
        if let Err(err) = persist_effective_node_select(state, generated.node_select).await {
            warn!(error = %err, "Failed to persist effective node_select after restart");
        }
    }

    Ok(RefreshOutcome {
        effect: RefreshEffect::Activated,
        runtime_update,
        node_select: generated.node_select,
        generated: Some(generated),
    })
}

pub(super) async fn regenerate_and_restart_runtime(
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
    // This is an explicit foreground refresh. Any startup fetch that began
    // earlier with the same subscription URLs must not publish after it.
    state.sub_refresh_generation.fetch_add(1, Ordering::Relaxed);
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

    let runtime_update = if should_run {
        match regenerate_and_restart_runtime(config, state, RefreshPolicy::Manual, SubSource::Fetch)
            .await
        {
            Ok(refresh) => {
                let outcome = refresh
                    .generated
                    .as_ref()
                    .expect("checked generated outcome");
                if refresh.effect == RefreshEffect::Activated {
                    finalize_started_config(config, state, outcome.has_sub_nodes).await;
                    refresh.runtime_update
                } else {
                    update_config_warning(config, state, outcome.has_sub_nodes).await;
                    state.runtime_ready.store(true, Ordering::Relaxed);
                    state.set_runtime_phase(RuntimePhase::Ready);
                    RuntimeUpdate::None
                }
            }
            Err(err) => {
                // Candidate validation never replaces config.json. Activation
                // may have, so rewind runtime + bindings before surfacing err.
                error!(error = %err, "Failed to refresh subscriptions, restoring previous runtime state");
                let restore = restore_after_apply_failure(
                    config,
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
        match regenerate_without_restart_runtime(config, state, SubSource::Fetch).await {
            Ok(outcome) => {
                persist_effective_node_select(state, outcome.node_select).await?;
                record_fresh_snapshot(config, state, &outcome).await;
                *state.skipped_rules.lock().await = outcome.skipped_rules.clone();
                update_config_warning(config, state, outcome.has_sub_nodes).await;
                state.set_runtime_phase(RuntimePhase::Stopped);
            }
            Err(err) => {
                error!(error = %err, "Failed to regenerate config, restoring previous runtime config");
                let _ = restore_after_apply_failure(
                    config,
                    state,
                    false,
                    snapshot.as_deref(),
                    bindings_snapshot.as_deref(),
                    false,
                )
                .await;
                return Err(err);
            }
        }
        RuntimeUpdate::None
    };

    Ok(runtime_update)
}

pub(super) async fn finalize_started_config(
    config: &Config,
    state: &Arc<AppState>,
    has_sub_nodes: bool,
) {
    update_config_warning(config, state, has_sub_nodes).await;

    spawn_restore_last_proxy(state);
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

pub(super) async fn regenerate_without_restart_runtime(
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
pub(super) enum ConfigApplyMode {
    Clear,
    Restart,
    RegenerateOnly,
}

pub(super) fn config_apply_mode(config: &Config, should_run: bool) -> ConfigApplyMode {
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

pub(super) fn no_usable_nodes_warning(config: &Config) -> String {
    if config.subs.is_empty() {
        NO_USABLE_MANUAL.to_string()
    } else {
        NO_USABLE_SUBS.to_string()
    }
}

pub(super) async fn persist_config_without_usable_nodes_at(
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
                    *state.skipped_rules.lock().await = outcome.skipped_rules.clone();
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
    let mut new_config = old_config.clone();
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
    let mut previous = NodeSelect::default();
    let update = apply_config_mutation(state, |config| {
        previous = config.node_select;
        config.node_select = node_select;
        Ok(())
    })
    .await?;
    let effective = state.config.read().await.node_select;
    Ok((previous, effective, update))
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

#[cfg(all(test, unix))]
mod transaction_tests {
    use std::{os::unix::fs::PermissionsExt, sync::atomic::Ordering, sync::Arc};

    use crate::{
        models::{Config, StableConfig},
        paths::RuntimePaths,
        services::singbox::{is_sing_box_running, start_sing_internal, stop_sing_internal},
        state::AppState,
    };

    use super::{
        apply_config_change, regenerate_preserving_service_state,
        regenerate_without_restart_runtime, RuntimeUpdate, SubSource,
    };

    fn manual_node(tag: &str) -> String {
        serde_json::json!({
            "type": "hysteria2",
            "tag": tag,
            "server": "127.0.0.1",
            "server_port": 443,
            "password": "secret"
        })
        .to_string()
    }

    #[tokio::test]
    async fn persistent_save_failure_reactivates_previous_runtime_and_restores_bindings() {
        let unique = format!(
            "miao-transaction-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let runtime_dir = root.join("runtime");
        let config_path = root.join("config.yaml");
        let volatile_path = root.join("volatile.yaml");
        tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
        // A directory at the target path deterministically makes the stable
        // config commit fail after runtime activation, even when tests run as root.
        tokio::fs::create_dir_all(&config_path).await.unwrap();

        let fake_kernel = runtime_dir.join("sing-box");
        tokio::fs::write(
            &fake_kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
        std::fs::set_permissions(&fake_kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

        let old_config = Config {
            nodes: vec![manual_node("old-node")],
            ..Config::default()
        };
        let runtime_paths = RuntimePaths::new(runtime_dir, &config_path);
        let state = Arc::new(
            AppState::with_config_layers(
                StableConfig::from(&old_config),
                old_config.clone(),
                config_path,
                volatile_path,
                runtime_paths,
            )
            .unwrap(),
        );
        let old_runtime = br#"{"marker":"old-runtime"}"#;
        let old_bindings = br#"{"marker":"old-bindings"}"#;
        tokio::fs::write(&state.runtime_paths.active_config, old_runtime)
            .await
            .unwrap();
        tokio::fs::write(&state.runtime_paths.node_bindings, old_bindings)
            .await
            .unwrap();
        start_sing_internal(&state).await.unwrap();
        assert_eq!(state.sing_generation.load(Ordering::Relaxed), 1);
        let original_pid = state
            .sing_process
            .lock()
            .await
            .as_ref()
            .and_then(|process| process.child.id())
            .unwrap();

        let mut new_config = old_config.clone();
        new_config.nodes.push(manual_node("new-node"));
        let result = apply_config_change(&state, &old_config, &new_config).await;

        assert!(result.is_err(), "the persistent config commit must fail");
        assert_eq!(
            tokio::fs::read(&state.runtime_paths.active_config)
                .await
                .unwrap(),
            old_runtime
        );
        assert_eq!(
            tokio::fs::read(&state.runtime_paths.node_bindings)
                .await
                .unwrap(),
            old_bindings
        );
        assert!(is_sing_box_running(&state).await);
        assert_eq!(
            state
                .sing_process
                .lock()
                .await
                .as_ref()
                .and_then(|process| process.child.id()),
            Some(original_pid),
            "Unix rollback should reactivate the previous config without replacing the process"
        );
        assert!(
            state.sing_generation.load(Ordering::Relaxed) >= 3,
            "both activation and rollback must retire their previous watchers"
        );
        assert_eq!(*state.config.read().await, old_config);

        stop_sing_internal(&state).await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn unchanged_runtime_bytes_still_start_a_missing_desired_process() {
        let unique = format!(
            "miao-unchanged-start-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let runtime_dir = root.join("runtime");
        let config_path = root.join("config.yaml");
        let volatile_path = root.join("volatile.yaml");
        tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
        let fake_kernel = runtime_dir.join("sing-box");
        tokio::fs::write(
            &fake_kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
        std::fs::set_permissions(&fake_kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

        let config = Config {
            nodes: vec![manual_node("only-node")],
            ..Config::default()
        };
        let runtime_paths = RuntimePaths::new(runtime_dir, &config_path);
        let state = Arc::new(
            AppState::with_config_layers(
                StableConfig::from(&config),
                config.clone(),
                config_path,
                volatile_path,
                runtime_paths,
            )
            .unwrap(),
        );

        regenerate_without_restart_runtime(&config, &state, SubSource::Fetch)
            .await
            .unwrap();
        assert!(!is_sing_box_running(&state).await);

        let runtime_update = regenerate_preserving_service_state(&config, &state)
            .await
            .unwrap();

        assert_eq!(runtime_update, RuntimeUpdate::Started);
        assert!(is_sing_box_running(&state).await);
        assert!(state.runtime_ready.load(Ordering::Relaxed));

        stop_sing_internal(&state).await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn config_update_lock_serializes_overlapping_node_adds() {
        let unique = format!(
            "miao-serialize-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let runtime_dir = root.join("runtime");
        let config_path = root.join("config.yaml");
        let volatile_path = root.join("volatile.yaml");
        tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
        let fake_kernel = runtime_dir.join("sing-box");
        tokio::fs::write(
            &fake_kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
        std::fs::set_permissions(&fake_kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

        let old_config = Config {
            nodes: vec![manual_node("base-node")],
            ..Config::default()
        };
        let runtime_paths = RuntimePaths::new(runtime_dir, &config_path);
        let state = Arc::new(
            AppState::with_config_layers(
                StableConfig::from(&old_config),
                old_config.clone(),
                config_path,
                volatile_path,
                runtime_paths,
            )
            .unwrap(),
        );
        start_sing_internal(&state).await.unwrap();

        let add = |state: Arc<AppState>, tag: &'static str| async move {
            let _guard = state.config_update.lock().await;
            let old = state.config.read().await.clone();
            let mut new = old.clone();
            new.nodes.push(manual_node(tag));
            apply_config_change(&state, &old, &new).await
        };

        let (first, second) =
            tokio::join!(add(state.clone(), "node-a"), add(state.clone(), "node-b"),);
        first.expect("first add should apply");
        second.expect("second add should apply");

        let tags: Vec<String> = state
            .config
            .read()
            .await
            .nodes
            .iter()
            .filter_map(|raw| {
                serde_json::from_str::<serde_json::Value>(raw)
                    .ok()?
                    .get("tag")?
                    .as_str()
                    .map(str::to_string)
            })
            .collect();
        assert!(tags.contains(&"base-node".to_string()));
        assert!(tags.contains(&"node-a".to_string()));
        assert!(tags.contains(&"node-b".to_string()));
        assert_eq!(tags.len(), 3);

        stop_sing_internal(&state).await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
