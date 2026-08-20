use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, NodeSelect};
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

/// 订阅刷新策略：机制（拉取 → 生成 → 校验 → 重启）只有一条，差异显式表达
pub enum RefreshPolicy {
    /// 手动刷新（面板「刷新订阅」等独立路径）：生成+校验成功后总是重启；
    /// 全部订阅失败也用现有结果继续；重启后由本管线持久化生效的 node_select
    Manual,
    /// apply_config_change 事务内的刷新：机制同 Manual，但 node_select 由外层
    /// 事务随新配置一并提交，本管线不提前写盘——避免「旧配置 + 新选择」的中间快照
    ManualInApply,
    /// 启动快速通道：与启动所用缓存比对无变化则不重启；
    /// 全部订阅失败/校验失败时保留正在运行的缓存配置
    Startup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshEffect {
    Restarted,
    SkippedUnchanged,
    KeptRunningOnTotalFailure,
    KeptRunningOnValidationFailure,
}

pub struct RefreshOutcome {
    pub effect: RefreshEffect,
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

/// 刷新后是否需要重启内核：与启动缓存逐字节不同才需要；读不出内容时保守重启
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

/// Validate and install a generated config for startup before any process is
/// launched. The caller publishes cache/snapshot state only after start.
pub async fn install_prepared_runtime(
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
) -> AppResult<()> {
    validate_prepared_config(state, &outcome.bytes).await?;
    let old_runtime = tokio::fs::read(&state.runtime_paths.active_config)
        .await
        .ok();
    let old_bindings = tokio::fs::read(&state.runtime_paths.node_bindings)
        .await
        .ok();
    let commit = async {
        write_file_atomic(&state.runtime_paths.active_config, &outcome.bytes).await?;
        save_node_bindings(state, &outcome.node_bindings).await
    }
    .await;
    if let Err(commit_err) = commit {
        let runtime_rollback =
            restore_file_snapshot(&state.runtime_paths.active_config, old_runtime.as_deref()).await;
        let bindings_rollback =
            restore_file_snapshot(&state.runtime_paths.node_bindings, old_bindings.as_deref())
                .await;
        if let Err(rollback_err) = runtime_rollback.and(bindings_rollback) {
            return Err(AppError::message(format!(
                "{}. Prepared config rollback failed: {}",
                commit_err, rollback_err
            )));
        }
        return Err(commit_err);
    }
    Ok(())
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
) -> AppResult<()> {
    let old_runtime = snapshot_runtime_config(state).await;
    let old_bindings = tokio::fs::read(&state.runtime_paths.node_bindings)
        .await
        .ok();
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
    Ok(())
}

/// 订阅刷新统一管线：获取节点集（source 决定真拉取还是快照重建）→ 生成配置 →（策略门控）→ 校验 → 重启内核。
/// 不包含缓存保存/节点恢复/告警文案——由调用方按 outcome 决定。
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

    let cache_bytes = read_config_cache(state).await;
    let generated = match source {
        SubSource::Fetch => gen_config(config, state, retry).await,
        SubSource::SnapshotOrFetch => gen_config_from_snapshot(config, state).await,
        SubSource::Prefetched(nodes) => gen_config_from_nodes(config, state, nodes).await,
    }
    .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    if startup && !generated.has_sub_nodes && !config.subs.is_empty() {
        return Ok(RefreshOutcome {
            effect: RefreshEffect::KeptRunningOnTotalFailure,
            node_select: generated.node_select,
            generated: None,
        });
    }

    if startup && !config_changed_after_refresh(cache_bytes.as_deref(), Some(&generated.bytes)) {
        info!("Subscriptions unchanged after refresh; sing-box keeps running");
        // 内容无变化等价于缓存那份（已通过校验）：顺带补齐快照
        save_node_bindings(state, &generated.node_bindings).await?;
        record_fresh_snapshot(config, state, &generated).await;
        persist_effective_node_select(state, generated.node_select).await?;
        *state.skipped_rules.lock().await = generated.skipped_rules.clone();
        return Ok(RefreshOutcome {
            effect: RefreshEffect::SkippedUnchanged,
            node_select: generated.node_select,
            generated: Some(generated),
        });
    }

    if let Err(e) = validate_prepared_config(state, &generated.bytes).await {
        if startup {
            return Ok(RefreshOutcome {
                effect: RefreshEffect::KeptRunningOnValidationFailure,
                node_select: generated.node_select,
                generated: None,
            });
        }
        return Err(AppError::context(
            "Config validation failed, not restarting",
            e,
        ));
    }
    activate_running_config(state, &generated).await?;
    info!("sing-box restarted successfully");
    // ManualInApply 的 node_select 由外层 apply_config_change 事务一并提交
    if !matches!(policy, RefreshPolicy::ManualInApply) {
        record_fresh_snapshot(config, state, &generated).await;
        *state.skipped_rules.lock().await = generated.skipped_rules.clone();
        if let Err(err) = persist_effective_node_select(state, generated.node_select).await {
            warn!(error = %err, "Failed to persist effective node_select after restart");
        }
    }

    Ok(RefreshOutcome {
        effect: RefreshEffect::Restarted,
        node_select: generated.node_select,
        generated: Some(generated),
    })
}

pub(super) async fn regenerate_and_restart_runtime(
    config: &Config,
    state: &Arc<AppState>,
    policy: RefreshPolicy,
    source: SubSource,
) -> AppResult<GenConfigOutcome> {
    let outcome = refresh_subscriptions(config, state, policy, source).await?;
    outcome
        .generated
        .ok_or_else(|| AppError::message("Runtime refresh kept the previous configuration"))
}

pub async fn regenerate_preserving_service_state(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let should_run = state.service_should_run.load(Ordering::Relaxed);

    if config_apply_mode(config, should_run) == ConfigApplyMode::Clear {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(false);
    }

    // 回滚 tier 1 材料：刷新前正在运行/最近可用的运行时配置字节
    let snapshot = snapshot_runtime_config(state).await;

    if should_run {
        match regenerate_and_restart_runtime(config, state, RefreshPolicy::Manual, SubSource::Fetch)
            .await
        {
            Ok(outcome) => {
                finalize_started_config(config, state, outcome.has_sub_nodes).await;
            }
            Err(err) => {
                // 校验失败时磁盘已是未通过校验的新配置而内核还在跑旧配置；
                // 重启失败时内核已停。先把运行时恢复到刷新前状态，再上报原错误。
                error!(error = %err, "Failed to refresh subscriptions, restoring previous runtime state");
                let restore =
                    restore_previous_running_config(config, state, snapshot.as_deref()).await;
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
            }
            Err(err) => {
                error!(error = %err, "Failed to regenerate config, restoring previous runtime config");
                let _ = restore_previous_stopped_config(config, state, snapshot.as_deref()).await;
                return Err(err);
            }
        }
    }

    Ok(should_run)
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
        Some("该地区没有可用节点，已切回手动选择".to_string())
    } else if has_sub_nodes {
        None
    } else if !config.subs.is_empty() {
        Some("所有订阅获取失败，请检查当前订阅".to_string())
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

async fn remove_runtime_config_files(state: &AppState) {
    remove_runtime_config_files_at(
        &state.runtime_paths.active_config,
        &state.runtime_paths.config_cache,
        &state.runtime_paths.sub_nodes_snapshot,
    )
    .await;
    if let Err(err) = tokio::fs::remove_file(&state.runtime_paths.cache_manifest).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(error = %err, "Failed to remove stale cache manifest");
        }
    }
}

async fn clear_runtime_config(state: &Arc<AppState>) {
    remove_runtime_config_files(state).await;
    state.sub_status.lock().await.clear();
    *state.config_warning.lock().await = None;
}

pub(super) fn no_usable_nodes_warning(config: &Config) -> String {
    if config.subs.is_empty() {
        "没有可用的手动节点，请检查配置或添加节点".to_string()
    } else {
        "所有订阅获取失败且没有可用手动节点，请检查订阅或添加节点".to_string()
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
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(());
    }

    if should_run {
        restore_previous_running_config(old_config, state, snapshot).await
    } else {
        restore_previous_stopped_config(old_config, state, snapshot).await
    }
}

pub async fn apply_config_change(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
) -> AppResult<()> {
    let should_run = state.service_should_run.load(Ordering::Relaxed);
    let apply_mode = config_apply_mode(new_config, should_run);

    if apply_mode == ConfigApplyMode::Clear {
        save_config_layered(state, new_config).await?;
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        *state.config.write().await = new_config.clone();
        *state.skipped_rules.lock().await = Vec::new();
        return Ok(());
    }

    // 回滚 tier 1 材料：变更前正在运行/最近可用的运行时配置字节（config.json）
    let snapshot = snapshot_runtime_config(state).await;
    // 订阅列表没变就是本地语义变更（节点选择/路由模式/规则/手动节点），走快照零网络重建
    let source = sub_source_for(old_config, new_config);

    let apply_result = match apply_mode {
        ConfigApplyMode::Restart => {
            regenerate_and_restart_runtime(new_config, state, RefreshPolicy::ManualInApply, source)
                .await
        }
        ConfigApplyMode::RegenerateOnly => {
            regenerate_without_restart_runtime(new_config, state, source).await
        }
        ConfigApplyMode::Clear => unreachable!("clear mode handled above"),
    };

    match apply_result {
        Ok(outcome) => {
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
                        finalize_started_config(new_config, state, outcome.has_sub_nodes).await;
                    } else {
                        update_config_warning(new_config, state, outcome.has_sub_nodes).await;
                    }
                    Ok(())
                }
                Err(save_err) => {
                    error!(error = %save_err, "Runtime config applied but persistent config write failed, attempting runtime rollback");
                    match restore_previous_config(
                        old_config,
                        state,
                        should_run,
                        snapshot.as_deref(),
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
                match restore_previous_config(old_config, state, should_run, snapshot.as_deref())
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
                persist_config_without_usable_nodes(state, new_config.clone()).await
            }
        }
        Err(apply_err) => {
            error!(error = %apply_err, "Failed to apply runtime config change, attempting runtime rollback");
            match restore_previous_config(old_config, state, should_run, snapshot.as_deref()).await
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
            Ok(true) => return Ok(()),
            Ok(false) => {
                // 本地没有任何材料（新装/清场后）：才退化到重新生成（网络）
                let outcome =
                    regenerate_without_restart_runtime(old_config, state, SubSource::Fetch).await?;
                update_config_warning(old_config, state, outcome.has_sub_nodes).await;
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
        match start_sing_internal(state).await {
            Ok(()) => {
                finalize_started_config(old_config, state, true).await;
                return Ok(());
            }
            Err(err) => {
                warn!(error = %err, source = source, "Failed to start sing-box from restored config, trying next source");
            }
        }
    }

    // 本地材料全部不可用/失败，才退化到重新生成（网络）
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
        return Ok(());
    }

    // 服务本就处于停止态：只需把磁盘修回变更前配置，不起进程；
    // 本地无材料才退化到重新生成（网络）
    match restore_disk_config(state, snapshot).await {
        Ok(true) => Ok(()),
        Ok(false) => {
            let outcome =
                regenerate_without_restart_runtime(old_config, state, SubSource::Fetch).await?;
            update_config_warning(old_config, state, outcome.has_sub_nodes).await;
            Ok(())
        }
        Err(err) => Err(err),
    }
}
