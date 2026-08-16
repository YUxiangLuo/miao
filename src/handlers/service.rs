use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Instant,
};
use tokio::time::Duration;

use crate::error::AppError;
use crate::models::{ApiResponse, ConnectivityResult, RouteMode, RouteModeRequest, StatusData};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::{
    config::apply_runtime_config_change,
    proxy::restore_last_proxy,
    singbox::{start_sing_internal, stop_sing_internal},
};
use crate::state::AppState;

pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<ApiResponse<StatusData>> {
    // 快速获取进程状态并立即释放锁
    let (running, pid, uptime_secs) = {
        let mut lock = state.sing_process.lock().await;

        if let Some(ref mut proc) = *lock {
            match proc.child.try_wait() {
                Ok(Some(_)) => {
                    *lock = None;
                    (false, None, None)
                }
                Ok(None) => {
                    let uptime = proc.started_at.elapsed().as_secs();
                    (true, proc.child.id(), Some(uptime))
                }
                Err(_) => (false, None, None),
            }
        } else {
            (false, None, None)
        }
    }; // sing_process 锁在此处释放

    let initializing = state
        .initializing
        .load(std::sync::atomic::Ordering::Relaxed);
    let mut warnings: Vec<String> = Vec::new();
    if let Some(warning) = state.config_warning.lock().await.clone() {
        warnings.push(warning);
    }
    let skipped_rules = state.skipped_rules.lock().await;
    if !skipped_rules.is_empty() {
        warnings.push(format!(
            "{} 条自定义规则因出口节点不存在已跳过: {}",
            skipped_rules.len(),
            skipped_rules
                .iter()
                .map(|rule| rule.description.as_str())
                .collect::<Vec<_>>()
                .join(";")
        ));
    }
    drop(skipped_rules);
    let warning = if warnings.is_empty() {
        None
    } else {
        Some(warnings.join(";"))
    };
    let route_mode = state
        .route_mode_override
        .read()
        .await
        .unwrap_or(RouteMode::default());
    let adblock = state.config.read().await.adblock;

    success(
        if running { "running" } else { "stopped" },
        StatusData {
            running,
            initializing,
            route_mode,
            adblock,
            pid,
            uptime_secs,
            warning,
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
            let state_for_proxy = state.clone();
            tokio::spawn(async move {
                restore_last_proxy(&state_for_proxy).await;
            });
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

    let _config_update = state.config_update.lock().await;
    let was_running = sing_box_is_running(&state).await;
    let old_config = state.config.read().await.clone();
    let current_route_mode = state
        .route_mode_override
        .read()
        .await
        .unwrap_or(RouteMode::default());

    if current_route_mode == req.route_mode {
        return Ok(success_no_data("Route mode unchanged"));
    }

    let mut old_runtime_config = old_config.clone();
    old_runtime_config.route_mode = current_route_mode;
    let mut new_runtime_config = old_config.clone();
    new_runtime_config.route_mode = req.route_mode;

    let result = apply_runtime_config_change(
        &state,
        &old_runtime_config,
        &new_runtime_config,
        was_running,
    )
    .await;

    match result {
        Ok(_) if was_running => Ok(success_no_data(
            "Route mode updated for current session and sing-box restarted",
        )),
        Ok(_) => Ok(success_no_data("Route mode updated for current session")),
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
            adblock: false,
            location: None,
        });

        let axum::response::Json(response) = get_status(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "stopped");
        let data = response.data.unwrap();
        assert!(!data.running);
        assert!(data.pid.is_none());
        assert!(data.uptime_secs.is_none());
    }

    #[tokio::test]
    async fn get_status_reports_route_mode_override_without_mutating_config() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: RouteMode::Rule,
            adblock: false,
            location: None,
        });
        *state.route_mode_override.write().await = Some(RouteMode::Global);

        let axum::response::Json(response) = get_status(State(state.clone())).await;

        let data = response.data.unwrap();
        assert_eq!(data.route_mode, RouteMode::Global);
        assert_eq!(state.config.read().await.route_mode, RouteMode::Rule);
    }

    #[tokio::test]
    async fn get_status_ignores_persisted_route_mode_without_override() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: RouteMode::Global,
            adblock: false,
            location: None,
        });

        let axum::response::Json(response) = get_status(State(state)).await;

        let data = response.data.unwrap();
        assert_eq!(data.route_mode, RouteMode::Rule);
    }
}
