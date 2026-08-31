use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;
use std::sync::{atomic::Ordering, Arc};
use std::time::Instant;
use tokio::time::Duration;

use crate::error::AppError;
use crate::models::{
    ApiResponse, ConnectivityResult, MaxMultiplierRequest, NodeMultiplier, NodeSelect,
    NodeSelectRequest, RouteModeRequest, StatusData,
};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::{
    proxy::spawn_restore_last_proxy,
    singbox::{kernel_status, start_sing_internal, stop_sing_internal},
    status::{legacy_warning, runtime_config_status, runtime_warnings},
};
use crate::state::AppState;

pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<ApiResponse<StatusData>> {
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
            running,
            ready: state.runtime_ready.load(Ordering::Relaxed),
            phase: state.runtime_phase(),
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

pub async fn start_service(State(state): State<Arc<AppState>>) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let _config_update = state.config_update.lock().await;
    let config = state.config.read().await;
    if config.subs.is_empty() && config.nodes.is_empty() {
        return Err(status_error(
            StatusCode::BAD_REQUEST,
            "Add a subscription or node before starting sing-box",
        ));
    }
    drop(config);

    // Record the user's desired state before launching. If startup fails, a
    // subsequent config fix should retry starting instead of silently keeping
    // the explicitly stopped state.
    state.service_should_run.store(true, Ordering::Relaxed);

    match start_sing_internal(&state).await {
        Ok(_) => {
            spawn_restore_last_proxy(&state);
            Ok(success_no_data("sing-box started successfully"))
        }
        Err(AppError::AlreadyRunning) => Err(status_error(
            StatusCode::BAD_REQUEST,
            "sing-box is already running",
        )),
        Err(e) => Err(status_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to start: {}", e),
        )),
    }
}

pub async fn stop_service(State(state): State<Arc<AppState>>) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let _config_update = state.config_update.lock().await;
    state.service_should_run.store(false, Ordering::Relaxed);
    stop_sing_internal(&state).await;
    Ok(success_no_data("sing-box stopped"))
}

pub async fn set_route_mode(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RouteModeRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    match crate::services::config::apply_route_mode(&state, req.route_mode).await {
        Ok((_, update)) if update.updated() => Ok(success_no_data("Route mode updated")),
        Ok(_) => Ok(success_no_data("Route mode unchanged")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn set_max_multiplier(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MaxMultiplierRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let max_multiplier = req
        .max_multiplier
        .as_deref()
        .map(|value| {
            NodeMultiplier::parse(value).ok_or_else(|| {
                status_error(
                    StatusCode::BAD_REQUEST,
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
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn set_node_select(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NodeSelectRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let node_select = NodeSelect::parse(&req.node_select).ok_or_else(|| {
        status_error(
            StatusCode::BAD_REQUEST,
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
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

#[derive(Deserialize)]
pub(crate) struct ConnectivityRequest {
    url: String,
}

pub async fn test_connectivity(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ConnectivityRequest>,
) -> Json<ApiResponse<ConnectivityResult>> {
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use axum::{extract::State, http::StatusCode};

    use super::{get_status, start_service, stop_service};
    use crate::models::{Config, RouteMode};
    use crate::test_support::app_state;

    #[tokio::test]
    async fn explicit_stop_updates_desired_service_state() {
        let state = app_state(Config::default());
        state.initializing.store(false, Ordering::Relaxed);

        assert!(stop_service(State(state.clone())).await.is_ok());

        assert!(!state.service_should_run.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn start_rejects_an_empty_configuration() {
        let state = app_state(Config::default());
        state.initializing.store(false, Ordering::Relaxed);
        state.service_should_run.store(false, Ordering::Relaxed);

        let status = match start_service(State(state.clone())).await {
            Ok(_) => panic!("empty configuration unexpectedly started"),
            Err((status, _)) => status,
        };

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!state.service_should_run.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn get_status_reports_stopped_when_no_process_exists() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        });

        let axum::response::Json(response) = get_status(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "stopped");
        let data = response.data.unwrap();
        assert!(!data.running);
        assert!(data.pid.is_none());
        assert!(data.uptime_secs.is_none());
        assert_eq!(data.vps_supported, crate::platform::vps_supported());
        assert_eq!(
            data.platform,
            if cfg!(windows) { "windows" } else { "linux" }
        );
    }

    #[tokio::test]
    async fn get_status_reports_dynamic_multiplier_options() {
        let selected = crate::models::NodeMultiplier::parse("2.5").unwrap();
        let state = app_state(Config {
            max_multiplier: Some(selected),
            ..Config::default()
        });
        *state.available_multipliers.write().await = vec![
            crate::models::NodeMultiplier::ONE,
            crate::models::NodeMultiplier::parse("6.5").unwrap(),
        ];
        *state.node_select_preference.write().await =
            crate::models::NodeSelect::Fastest(crate::models::Region::Jp);

        let axum::response::Json(response) = get_status(State(state)).await;
        let data = response.data.unwrap();

        assert_eq!(data.max_multiplier.as_deref(), Some("2.5"));
        assert_eq!(
            data.requested_node_select,
            crate::models::NodeSelect::Fastest(crate::models::Region::Jp)
        );
        assert_eq!(data.multiplier_options, ["1", "2.5", "6.5"]);
    }

    #[tokio::test]
    async fn get_status_reports_route_mode_from_config() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: RouteMode::Global,
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        });

        let axum::response::Json(response) = get_status(State(state)).await;

        let data = response.data.unwrap();
        assert_eq!(data.route_mode, RouteMode::Global);
    }
}
