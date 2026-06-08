use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Instant,
};
use tokio::time::Duration;

use crate::error::AppError;
use crate::models::{ApiResponse, ConnectivityResult, StatusData};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::{
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
    let warning = state.config_warning.lock().await.clone();

    success(
        if running { "running" } else { "stopped" },
        StatusData {
            running,
            initializing,
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
            "Service is initializing, please wait",
        ));
    }

    let _config_update = state.config_update.lock().await;

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
            "Service is initializing, please wait",
        ));
    }

    let _config_update = state.config_update.lock().await;

    stop_sing_internal(&state).await;
    Ok(success_no_data("sing-box stopped"))
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
    use axum::{extract::State, http::StatusCode, response::Json};
    use std::sync::atomic::Ordering;
    use tokio::time::{timeout, Duration};

    use super::{get_status, start_service, stop_service};
    use crate::models::Config;
    use crate::test_support::app_state;

    #[tokio::test]
    async fn get_status_reports_stopped_when_no_process_exists() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            vps_ip: None,
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
    async fn start_service_rejects_requests_while_initializing() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            vps_ip: None,
        });

        let result = start_service(State(state)).await;

        let Err((status, Json(response))) = result else {
            panic!("start_service should reject while initializing");
        };
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(!response.success);
        assert_eq!(response.message, "Service is initializing, please wait");
    }

    #[tokio::test]
    async fn stop_service_rejects_requests_while_initializing() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            vps_ip: None,
        });

        let result = stop_service(State(state)).await;

        let Err((status, Json(response))) = result else {
            panic!("stop_service should reject while initializing");
        };
        assert_eq!(status, StatusCode::CONFLICT);
        assert!(!response.success);
        assert_eq!(response.message, "Service is initializing, please wait");
    }

    #[tokio::test]
    async fn start_service_waits_for_config_update_lock() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            vps_ip: None,
        });
        state.initializing.store(false, Ordering::Relaxed);
        let call_state = state.clone();
        let _config_update = state.config_update.lock().await;

        let result = timeout(Duration::from_millis(50), start_service(State(call_state))).await;

        assert!(
            result.is_err(),
            "start_service should wait for config_update"
        );
    }

    #[tokio::test]
    async fn stop_service_waits_for_config_update_lock() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            vps_ip: None,
        });
        state.initializing.store(false, Ordering::Relaxed);
        let call_state = state.clone();
        let _config_update = state.config_update.lock().await;

        let result = timeout(Duration::from_millis(50), stop_service(State(call_state))).await;

        assert!(
            result.is_err(),
            "stop_service should wait for config_update"
        );
    }
}
