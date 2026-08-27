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
    /// 结果继续。node_select、节点快照与诊断状态由外层事务统一提交。
    Manual,
    /// apply_config_change 事务内的刷新：机制同 Manual；持久配置和诊断状态
    /// 同样由外层事务提交，避免发布半完成的中间状态。
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
        // Foreground refreshes belong to an outer transaction which publishes
        // preferences and diagnostics only after persistence succeeds.
        if matches!(policy, RefreshPolicy::Startup) {
            persist_effective_node_select(state, generated.node_select).await?;
            record_fresh_snapshot(config, state, &generated).await;
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
    // Foreground callers commit node_select and diagnostics transactionally.
    // Startup remains availability-first after a successful activation.
    if matches!(policy, RefreshPolicy::Startup) {
        if let Err(err) = persist_effective_node_select(state, generated.node_select).await {
            warn!(error = %err, "Failed to persist effective node_select after startup refresh");
        }
        record_fresh_snapshot(config, state, &generated).await;
        *state.skipped_rules.lock().await = generated.skipped_rules.clone();
    }

    Ok(RefreshOutcome {
        effect: RefreshEffect::Activated,
        runtime_update,
        node_select: generated.node_select,
        generated: Some(generated),
    })
}

mod transaction;

pub use transaction::{
    apply_config_change, apply_disabled_nodes, apply_node_select, apply_route_mode,
    regenerate_preserving_service_state, ConfigMutationError,
};
#[cfg(test)]
pub(super) use transaction::{
    config_apply_mode, no_usable_nodes_warning, persist_config_without_usable_nodes_at,
    regenerate_without_restart_runtime, ConfigApplyMode,
};

#[cfg(all(test, unix))]
mod transaction_tests;
