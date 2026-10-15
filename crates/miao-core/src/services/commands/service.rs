use serde::{Deserialize, Serialize};
use std::sync::{atomic::Ordering, Arc};
use std::time::Instant;
use tokio::time::Duration;

use super::{
    command_error, success, success_no_data, CommandErrorKind, CommandReply, CommandResult,
};
use crate::error::AppError;
use crate::models::{
    ConnectivityResult, MaxMultiplierRequest, NodeMultiplier, NodeSelect, NodeSelectRequest,
    RouteModeRequest, RuntimePhase, StatusData,
};
use crate::services::{
    config::RuntimeUpdate,
    proxy::spawn_restore_last_proxy,
    singbox::{extract_sing_box_to, kernel_status, start_sing_internal, stop_sing_internal},
    status::{legacy_warning, runtime_config_status, runtime_warnings},
};
use crate::state::AppState;

pub async fn get_status(state: Arc<AppState>) -> CommandReply<StatusData> {
    let kernel = kernel_status(&state).await;
    let (running, pid, uptime_secs) = (kernel.running, kernel.pid, kernel.uptime_secs);

    let initializing = state
        .initializing
        .load(std::sync::atomic::Ordering::Relaxed);
    let warnings = runtime_warnings(&state).await;
    let warning = legacy_warning(&warnings);
    let config = runtime_config_status(&state).await;

    success(
        if running { "running" } else { "stopped" },
        StatusData {
            data_revision: state.data_revision.load(Ordering::Relaxed),
            running,
            ready: kernel.ready,
            phase: kernel.phase,
            subscription_refresh: state.subscription_refresh.snapshot(),
            initializing,
            route_mode: config.route_mode,
            node_select: config.node_select,
            requested_node_select: config.requested_node_select,
            max_multiplier: config.max_multiplier.map(|value| value.as_config_value()),
            multiplier_options: config
                .multiplier_options
                .into_iter()
                .map(|value| value.as_config_value())
                .collect(),
            pid,
            uptime_secs,
            warning,
            warnings,
            vps_supported: true,
            platform: "linux",
            mcp: config.mcp,
        },
    )
}

pub async fn start_service(state: Arc<AppState>) -> CommandResult {
    super::ensure_initialized(&state)?;

    let config_update = state.config_update.lock().await;
    let config = state.config.read().await;
    if config.subs.is_empty() && config.nodes.is_empty() {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "Add a subscription or node before starting sing-box",
        ));
    }
    drop(config);

    // Record the user's desired state before launching. If startup fails, a
    // subsequent config fix should retry starting instead of silently keeping
    // the explicitly stopped state.
    state.lifecycle.request_running(true);

    if state.lifecycle.snapshot().phase == RuntimePhase::Failed {
        let generation = state.lifecycle.snapshot().generation;
        let activity = state
            .lifecycle
            .activity(crate::state::lifecycle::RuntimeActivity::Extracting);
        let runtime_dir = state.runtime_paths.runtime_dir.clone();
        let extracted =
            tokio::task::spawn_blocking(move || extract_sing_box_to(&runtime_dir)).await;
        match extracted {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                state.lifecycle.finish(generation, RuntimePhase::Failed);
                return Err(command_error(
                    CommandErrorKind::Internal,
                    format!("Failed to prepare embedded runtime: {err}"),
                ));
            }
            Err(err) => {
                state.lifecycle.finish(generation, RuntimePhase::Failed);
                return Err(command_error(
                    CommandErrorKind::Internal,
                    format!("Embedded runtime extraction task failed: {err}"),
                ));
            }
        }

        // The recovery helper fetches subscriptions without this lock and takes
        // it again only while publishing a current result.
        drop(activity);
        drop(config_update);
        if crate::runtime::recover_data_plane_once(&state).await && state.lifecycle.snapshot().ready
        {
            crate::services::version::mark_upgrade_healthy();
            return Ok(success_no_data("sing-box recovered successfully"));
        }
        return Err(command_error(
            CommandErrorKind::Internal,
            "Failed to recover sing-box runtime",
        ));
    }

    match start_sing_internal(&state).await {
        Ok(_) => {
            spawn_restore_last_proxy(&state);
            Ok(success_no_data("sing-box started successfully"))
        }
        Err(AppError::AlreadyRunning) => Err(command_error(
            CommandErrorKind::InvalidInput,
            "sing-box is already running",
        )),
        Err(e) => Err(command_error(
            CommandErrorKind::Internal,
            format!("Failed to start: {}", e),
        )),
    }
}

pub async fn stop_service(state: Arc<AppState>) -> CommandResult {
    super::ensure_initialized(&state)?;

    let _config_update = state.config_update.lock().await;
    state.next_sub_refresh();
    for status in state.sub_status.lock().await.values_mut() {
        if status.state == crate::models::SubscriptionState::Refreshing {
            status.state = if status.success {
                crate::models::SubscriptionState::Ready
            } else {
                crate::models::SubscriptionState::Failed
            };
        }
    }
    state.data_revision.fetch_add(1, Ordering::Relaxed);
    state.lifecycle.request_running(false);
    stop_sing_internal(&state).await;
    Ok(success_no_data("sing-box stopped"))
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RuntimeUpdateResult {
    pub runtime_updated: bool,
    pub started: bool,
    pub reloaded: bool,
    pub restarted: bool,
}

impl From<RuntimeUpdate> for RuntimeUpdateResult {
    fn from(update: RuntimeUpdate) -> Self {
        Self {
            runtime_updated: update.updated(),
            started: update == RuntimeUpdate::Started,
            reloaded: update == RuntimeUpdate::Reloaded,
            restarted: update == RuntimeUpdate::Restarted,
        }
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteModeResult {
    #[cfg_attr(test, ts(type = "RouteMode"))]
    pub route_mode: &'static str,
    pub changed: bool,
    #[serde(flatten)]
    pub runtime: RuntimeUpdateResult,
}

pub async fn set_route_mode(
    state: Arc<AppState>,
    req: RouteModeRequest,
) -> CommandResult<RouteModeResult> {
    super::ensure_initialized(&state)?;

    match crate::services::config::apply_route_mode(&state, req.route_mode).await {
        Ok((previous, update)) => {
            let changed = previous != req.route_mode;
            Ok(success(
                if changed || update.updated() {
                    "Route mode updated"
                } else {
                    "Route mode unchanged"
                },
                RouteModeResult {
                    route_mode: match req.route_mode {
                        crate::models::RouteMode::Rule => "rule",
                        crate::models::RouteMode::Global => "global",
                    },
                    changed,
                    runtime: update.into(),
                },
            ))
        }
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MaxMultiplierResult {
    pub max_multiplier: Option<String>,
    pub changed: bool,
    #[serde(flatten)]
    pub runtime: RuntimeUpdateResult,
}

pub async fn set_max_multiplier(
    state: Arc<AppState>,
    req: MaxMultiplierRequest,
) -> CommandResult<MaxMultiplierResult> {
    super::ensure_initialized(&state)?;

    let max_multiplier = req
        .max_multiplier
        .as_deref()
        .map(|value| {
            NodeMultiplier::parse(value).ok_or_else(|| {
                command_error(
                    CommandErrorKind::InvalidInput,
                    "最高倍率必须是大于 0 且不超过 10000 的十进制数，或使用 null 表示不限",
                )
            })
        })
        .transpose()?;

    match crate::services::config::apply_max_multiplier(&state, max_multiplier).await {
        Ok((previous, update)) => {
            let changed = previous != max_multiplier;
            Ok(success(
                if changed || update.updated() {
                    "Max multiplier updated"
                } else {
                    "Max multiplier unchanged"
                },
                MaxMultiplierResult {
                    max_multiplier: max_multiplier.map(|value| value.to_string()),
                    changed,
                    runtime: update.into(),
                },
            ))
        }
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeSelectResult {
    #[cfg_attr(test, ts(type = "NodeSelect"))]
    pub node_select: &'static str,
    #[cfg_attr(test, ts(type = "NodeSelect"))]
    pub requested: &'static str,
    pub changed: bool,
    #[serde(skip)]
    pub fell_back_to_manual: bool,
    #[serde(flatten)]
    pub runtime: RuntimeUpdateResult,
}

pub async fn set_node_select(
    state: Arc<AppState>,
    req: NodeSelectRequest,
) -> CommandResult<NodeSelectResult> {
    super::ensure_initialized(&state)?;

    let node_select = NodeSelect::parse(&req.node_select).ok_or_else(|| {
        command_error(
            CommandErrorKind::InvalidInput,
            "不支持的节点选择，可选: manual / fastest_hk / fastest_jp / fastest_tw / fastest_sg / fastest_us",
        )
    })?;

    match crate::services::config::apply_node_select(&state, node_select).await {
        Ok((previous, effective, update)) => {
            let fell_back_to_manual = !node_select.is_manual() && effective.is_manual();
            let changed = previous != node_select;
            let message = if fell_back_to_manual {
                crate::services::config::REGION_FALLBACK
            } else if changed || update.updated() || effective != node_select {
                "Node select updated"
            } else {
                "Node select unchanged"
            };
            Ok(success(
                message,
                NodeSelectResult {
                    node_select: effective.as_str(),
                    requested: node_select.as_str(),
                    changed,
                    fell_back_to_manual,
                    runtime: update.into(),
                },
            ))
        }
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

#[derive(Deserialize)]
pub(crate) struct ConnectivityRequest {
    pub(crate) url: String,
}

pub async fn test_connectivity(
    state: Arc<AppState>,
    req: ConnectivityRequest,
) -> CommandReply<ConnectivityResult> {
    let start = Instant::now();
    let result = match state
        .http_client
        .head(&req.url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) => ConnectivityResult {
            name: String::new(),
            url: req.url,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            success: true,
            http_status: Some(response.status().as_u16()),
            error_kind: None,
            error: None,
        },
        Err(error) => ConnectivityResult {
            name: String::new(),
            url: req.url,
            latency_ms: None,
            success: false,
            http_status: None,
            error_kind: Some(
                if error.is_timeout() {
                    "timeout"
                } else if error.is_connect() {
                    "connect"
                } else {
                    "request"
                }
                .into(),
            ),
            error: Some(error.without_url().to_string()),
        },
    };

    success("Test completed", result)
}
