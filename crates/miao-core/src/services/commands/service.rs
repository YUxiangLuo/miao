use serde::Deserialize;
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
    let config_status = runtime_config_status(&state).await;
    let config = config_status.config;

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
            requested_node_select: config_status.requested_node_select,
            max_multiplier: config.max_multiplier.map(|value| value.as_config_value()),
            multiplier_options: config_status
                .multiplier_options
                .into_iter()
                .map(|value| value.as_config_value())
                .collect(),
            pid,
            uptime_secs,
            warning,
            warnings,
            vps_supported: crate::platform::vps_supported(),
            platform: if cfg!(windows) { "windows" } else { "linux" },
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

pub async fn set_route_mode(state: Arc<AppState>, req: RouteModeRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    match crate::services::config::apply_route_mode(&state, req.route_mode).await {
        Ok((_, update)) if update.updated() => Ok(success_no_data("Route mode updated")),
        Ok(_) => Ok(success_no_data("Route mode unchanged")),
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

pub async fn set_max_multiplier(state: Arc<AppState>, req: MaxMultiplierRequest) -> CommandResult {
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
        Ok((previous, update)) if previous != max_multiplier || update.updated() => {
            Ok(success_no_data("Max multiplier updated"))
        }
        Ok(_) => Ok(success_no_data("Max multiplier unchanged")),
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

pub async fn set_node_select(state: Arc<AppState>, req: NodeSelectRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    let node_select = NodeSelect::parse(&req.node_select).ok_or_else(|| {
        command_error(
            CommandErrorKind::InvalidInput,
            "不支持的节点选择，可选: manual / fastest_hk / fastest_jp / fastest_tw / fastest_sg / fastest_us",
        )
    })?;

    match crate::services::config::apply_node_select(&state, node_select).await {
        Ok((previous, effective, update)) => {
            if !node_select.is_manual() && effective.is_manual() {
                Ok(success_no_data(crate::services::config::REGION_FALLBACK))
            } else if previous != node_select || update.updated() || effective != node_select {
                Ok(success_no_data("Node select updated"))
            } else {
                Ok(success_no_data("Node select unchanged"))
            }
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
        Ok(_) => ConnectivityResult {
            name: String::new(),
            url: req.url,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            success: true,
        },
        Err(_) => ConnectivityResult {
            name: String::new(),
            url: req.url,
            latency_ms: None,
            success: false,
        },
    };

    success("Test completed", result)
}
