use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, NodeSelect, RouteMode};
use crate::services::{
    proxy::restore_last_proxy,
    singbox::{
        get_sing_box_home, start_sing_internal, stop_sing_internal, validate_sing_box_config,
    },
};
use crate::state::AppState;

use super::generate::{gen_config, GenConfigOutcome, SubFetchRetry};
use super::persist::{
    config_cache_path, persist_effective_node_select, read_config_cache, restore_config_from_cache,
    save_config_cache, save_config_to,
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
    pub has_sub_nodes: bool,
    pub effect: RefreshEffect,
    pub node_select: NodeSelect,
}

/// 刷新后是否需要重启内核：与启动缓存逐字节不同才需要；读不出内容时保守重启
pub(super) fn config_changed_after_refresh(cache: Option<&[u8]>, current: Option<&[u8]>) -> bool {
    match (cache, current) {
        (Some(old), Some(new)) => old != new,
        _ => true,
    }
}

/// Startup 策略决定「保留运行中的缓存配置」时，gen_config 已把新配置写进
/// config.json（可能是地区筛空后的降级 selector，或未通过校验的版本）。运行中的
/// 内核不受影响，但崩溃看门狗与手动停/启都直接用磁盘 config.json 起进程——
/// 把缓存拷回，让磁盘文件与正在运行的内核保持一致。
async fn restore_cache_over_generated_config() {
    if let Err(err) = restore_config_from_cache().await {
        warn!(error = %err, "Failed to restore config.json from cache while keeping the running config");
    }
}

/// 订阅刷新统一管线：拉取订阅 → 生成配置 →（策略门控）→ 校验 → 重启内核。
/// 不包含缓存保存/节点恢复/告警文案——由调用方按 outcome 决定。
pub async fn refresh_subscriptions(
    config: &Config,
    state: &Arc<AppState>,
    policy: RefreshPolicy,
) -> AppResult<RefreshOutcome> {
    let startup = matches!(policy, RefreshPolicy::Startup);
    // 启动路径的订阅全失败多为「先于路由/DHCP 就绪」的瞬态，给退避预算；
    // 手动刷新用户在场，失败即报，不重试
    let retry = if startup {
        SubFetchRetry::Startup
    } else {
        SubFetchRetry::None
    };

    let cache_bytes = read_config_cache().await;
    let generated = gen_config(config, state, retry)
        .await
        .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    if startup && !generated.has_sub_nodes && !config.subs.is_empty() {
        restore_cache_over_generated_config().await;
        return Ok(RefreshOutcome {
            has_sub_nodes: generated.has_sub_nodes,
            effect: RefreshEffect::KeptRunningOnTotalFailure,
            node_select: generated.node_select,
        });
    }

    if startup {
        let current_bytes = tokio::fs::read(get_sing_box_home().join("config.json"))
            .await
            .ok();
        if !config_changed_after_refresh(cache_bytes.as_deref(), current_bytes.as_deref()) {
            info!("Subscriptions unchanged after refresh; sing-box keeps running");
            persist_effective_node_select(state, generated.node_select).await?;
            return Ok(RefreshOutcome {
                has_sub_nodes: generated.has_sub_nodes,
                effect: RefreshEffect::SkippedUnchanged,
                node_select: generated.node_select,
            });
        }
    }

    if let Err(e) = validate_sing_box_config().await {
        if startup {
            restore_cache_over_generated_config().await;
            return Ok(RefreshOutcome {
                has_sub_nodes: generated.has_sub_nodes,
                effect: RefreshEffect::KeptRunningOnValidationFailure,
                node_select: generated.node_select,
            });
        }
        return Err(AppError::context(
            "Config validation failed, not restarting",
            e,
        ));
    }

    stop_sing_internal(state).await;
    start_sing_internal(state)
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box", e))?;
    info!("sing-box restarted successfully");
    // ManualInApply 的 node_select 由外层 apply_config_change 事务一并提交
    if !matches!(policy, RefreshPolicy::ManualInApply) {
        if let Err(err) = persist_effective_node_select(state, generated.node_select).await {
            warn!(error = %err, "Failed to persist effective node_select after restart");
        }
    }

    Ok(RefreshOutcome {
        has_sub_nodes: generated.has_sub_nodes,
        effect: RefreshEffect::Restarted,
        node_select: generated.node_select,
    })
}

pub(super) async fn regenerate_and_restart_runtime(
    config: &Config,
    state: &Arc<AppState>,
    policy: RefreshPolicy,
) -> AppResult<GenConfigOutcome> {
    let outcome = refresh_subscriptions(config, state, policy).await?;
    Ok(GenConfigOutcome {
        has_sub_nodes: outcome.has_sub_nodes,
        node_select: outcome.node_select,
    })
}

pub async fn regenerate_preserving_service_state(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let route_override = *state.route_mode_override.read().await;
    let runtime_config = config_with_route_override(config, route_override);
    let should_run = state.service_should_run.load(Ordering::Relaxed);

    if config_apply_mode(&runtime_config, should_run) == ConfigApplyMode::Clear {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(false);
    }

    if should_run {
        let outcome =
            regenerate_and_restart_runtime(&runtime_config, state, RefreshPolicy::Manual).await?;
        finalize_started_config(&runtime_config, state, outcome.has_sub_nodes).await;
    } else {
        let outcome = regenerate_without_restart_runtime(&runtime_config, state).await?;
        persist_effective_node_select(state, outcome.node_select).await?;
        update_config_warning(&runtime_config, state, outcome.has_sub_nodes).await;
    }

    Ok(should_run)
}

pub(super) async fn finalize_started_config(
    config: &Config,
    state: &Arc<AppState>,
    has_sub_nodes: bool,
) {
    update_config_warning(config, state, has_sub_nodes).await;

    let state_for_proxy = state.clone();
    tokio::spawn(async move {
        restore_last_proxy(&state_for_proxy).await;
    });
}

async fn update_config_warning(config: &Config, state: &Arc<AppState>, has_sub_nodes: bool) {
    save_config_cache().await;

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
) -> AppResult<GenConfigOutcome> {
    let outcome = gen_config(config, state, SubFetchRetry::None)
        .await
        .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    validate_sing_box_config()
        .await
        .map_err(|e| AppError::context("Config validation failed", e))?;

    Ok(outcome)
}

pub(super) fn config_with_route_override(config: &Config, route_mode: Option<RouteMode>) -> Config {
    let mut config = config.clone();
    config.route_mode = route_mode.unwrap_or_default();
    config
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

async fn remove_runtime_config_files_at(runtime_config_path: &Path, cache_path: &Path) {
    for path in [runtime_config_path, cache_path] {
        if let Err(err) = tokio::fs::remove_file(path).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(path = ?path, error = %err, "Failed to remove stale runtime config");
            }
        }
    }
}

async fn remove_runtime_config_files() {
    let runtime_config_path = get_sing_box_home().join("config.json");
    let cache_path = config_cache_path();
    remove_runtime_config_files_at(&runtime_config_path, &cache_path).await;
}

async fn clear_runtime_config(state: &Arc<AppState>) {
    remove_runtime_config_files().await;
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
) -> AppResult<()> {
    save_config_to(&state.config_path, &persisted_config).await?;
    stop_sing_internal(state).await;
    remove_runtime_config_files_at(runtime_config_path, cache_path).await;
    *state.config.write().await = persisted_config.clone();
    *state.config_warning.lock().await = Some(no_usable_nodes_warning(&persisted_config));
    Ok(())
}

async fn persist_config_without_usable_nodes(
    state: &Arc<AppState>,
    persisted_config: Config,
) -> AppResult<()> {
    let runtime_config_path = get_sing_box_home().join("config.json");
    let cache_path = config_cache_path();
    persist_config_without_usable_nodes_at(
        state,
        persisted_config,
        &runtime_config_path,
        &cache_path,
    )
    .await
}

async fn restore_previous_config(
    old_config: &Config,
    state: &Arc<AppState>,
    should_run: bool,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(());
    }

    if should_run {
        restore_previous_running_config(old_config, state).await
    } else {
        restore_previous_stopped_config(old_config, state).await
    }
}

pub async fn apply_config_change(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
) -> AppResult<()> {
    let route_override = *state.route_mode_override.read().await;
    let runtime_old_config = config_with_route_override(old_config, route_override);
    let runtime_new_config = config_with_route_override(new_config, route_override);
    let persisted_new_config = config_with_route_override(new_config, None);
    let should_run = state.service_should_run.load(Ordering::Relaxed);
    let apply_mode = config_apply_mode(&runtime_new_config, should_run);

    if apply_mode == ConfigApplyMode::Clear {
        save_config_to(&state.config_path, &persisted_new_config).await?;
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        *state.config.write().await = persisted_new_config;
        *state.skipped_rules.lock().await = Vec::new();
        return Ok(());
    }

    let apply_result = match apply_mode {
        ConfigApplyMode::Restart => {
            regenerate_and_restart_runtime(&runtime_new_config, state, RefreshPolicy::ManualInApply)
                .await
        }
        ConfigApplyMode::RegenerateOnly => {
            regenerate_without_restart_runtime(&runtime_new_config, state).await
        }
        ConfigApplyMode::Clear => unreachable!("clear mode handled above"),
    };

    match apply_result {
        Ok(outcome) => {
            let mut persisted_new_config = persisted_new_config;
            persisted_new_config.node_select = outcome.node_select;
            match save_config_to(&state.config_path, &persisted_new_config).await {
                Ok(()) => {
                    *state.config.write().await = persisted_new_config;
                    if should_run {
                        finalize_started_config(&runtime_new_config, state, outcome.has_sub_nodes)
                            .await;
                    } else {
                        update_config_warning(&runtime_new_config, state, outcome.has_sub_nodes)
                            .await;
                    }
                    Ok(())
                }
                Err(save_err) => {
                    error!(error = %save_err, "Runtime config applied but persistent config write failed, attempting runtime rollback");
                    match restore_previous_config(&runtime_old_config, state, should_run).await {
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
            warn!(error = %apply_err, "Config change left no usable nodes; persisting it and stopping sing-box");
            persist_config_without_usable_nodes(state, persisted_new_config).await
        }
        Err(apply_err) => {
            error!(error = %apply_err, "Failed to apply runtime config change, attempting runtime rollback");
            match restore_previous_config(&runtime_old_config, state, should_run).await {
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

pub async fn apply_runtime_config_change(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
    restart: bool,
) -> AppResult<()> {
    if restart {
        match regenerate_and_restart_runtime(new_config, state, RefreshPolicy::Manual).await {
            Ok(outcome) => {
                *state.route_mode_override.write().await = Some(new_config.route_mode);
                finalize_started_config(new_config, state, outcome.has_sub_nodes).await;
                Ok(())
            }
            Err(apply_err) => {
                error!(error = %apply_err, "Failed to apply runtime-only config change, attempting runtime rollback");
                match restore_previous_running_config(old_config, state).await {
                    Ok(()) => Err(AppError::context(
                        "Failed to apply runtime-only config change; restored previous runtime config",
                        apply_err,
                    )),
                    Err(rollback_err) => Err(AppError::message(format!(
                        "Failed to apply runtime-only config change: {}. Runtime rollback failed: {}",
                        apply_err, rollback_err
                    ))),
                }
            }
        }
    } else {
        match regenerate_without_restart_runtime(new_config, state).await {
            Ok(outcome) => {
                *state.route_mode_override.write().await = Some(new_config.route_mode);
                persist_effective_node_select(state, outcome.node_select).await?;
                update_config_warning(new_config, state, outcome.has_sub_nodes).await;
                Ok(())
            }
            Err(apply_err) => {
                let _ = restore_previous_stopped_config(old_config, state).await;
                Err(AppError::context(
                    "Failed to apply runtime-only config change",
                    apply_err,
                ))
            }
        }
    }
}

async fn sing_box_is_running(state: &Arc<AppState>) -> bool {
    let mut lock = state.sing_process.lock().await;
    match &mut *lock {
        Some(proc) => match proc.child.try_wait() {
            Ok(Some(_)) => {
                *lock = None;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        },
        None => false,
    }
}

async fn restore_previous_running_config(
    old_config: &Config,
    state: &Arc<AppState>,
) -> AppResult<()> {
    if sing_box_is_running(state).await {
        match restore_config_from_cache().await {
            Ok(()) => {}
            Err(cache_err) => {
                warn!(error = %cache_err, "Failed to restore runtime config from cache while previous sing-box process is still running");
                let outcome = regenerate_without_restart_runtime(old_config, state).await?;
                update_config_warning(old_config, state, outcome.has_sub_nodes).await;
            }
        }
        return Ok(());
    }

    restart_with_previous_config(old_config, state).await
}

async fn restart_with_previous_config(old_config: &Config, state: &Arc<AppState>) -> AppResult<()> {
    stop_sing_internal(state).await;

    if let Err(cache_err) = restore_config_from_cache().await {
        warn!(error = %cache_err, "Failed to restore runtime config from cache for rollback; regenerating previous config");
    } else {
        match start_sing_internal(state).await {
            Ok(()) => {
                finalize_started_config(old_config, state, true).await;
                return Ok(());
            }
            Err(start_err) => {
                warn!(error = %start_err, "Failed to restart sing-box from cached config; regenerating previous config");
            }
        }
    }

    let outcome = regenerate_without_restart_runtime(old_config, state).await?;
    start_sing_internal(state)
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box with previous config", e))?;
    finalize_started_config(old_config, state, outcome.has_sub_nodes).await;
    Ok(())
}

async fn restore_previous_stopped_config(
    old_config: &Config,
    state: &Arc<AppState>,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        clear_runtime_config(state).await;
        return Ok(());
    }

    let outcome = regenerate_without_restart_runtime(old_config, state).await?;
    update_config_warning(old_config, state, outcome.has_sub_nodes).await;
    Ok(())
}
