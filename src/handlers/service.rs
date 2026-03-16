use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;
use std::{sync::Arc, time::Instant};
use tokio::time::Duration;

use crate::error::AppError;
use crate::models::{ApiResponse, ConnectivityResult, StatusData};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::{
    proxy::restore_last_proxy,
    singbox::{start_sing_internal, stop_sing_internal},
};
use crate::state::{AppState, CLIENT, CONFIG_WARNING, INITIALIZING, SING_PROCESS};

pub async fn get_status() -> Json<ApiResponse<StatusData>> {
    let mut lock = SING_PROCESS.lock().await;

    let (running, pid, uptime_secs) = if let Some(ref mut proc) = *lock {
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
    };

    let initializing = INITIALIZING.load(std::sync::atomic::Ordering::Relaxed);
    let warning = CONFIG_WARNING.lock().await.clone();

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

pub async fn start_service(
    State(_): State<Arc<AppState>>,
) -> HandlerResult {
    match start_sing_internal().await {
        Ok(_) => {
            tokio::spawn(async {
                restore_last_proxy().await;
            });
            Ok(success_no_data("sing-box started successfully"))
        }
        Err(AppError::AlreadyRunning) => {
            Err(status_error(StatusCode::BAD_REQUEST, "sing-box is already running"))
        }
        Err(e) => {
            Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to start: {}", e)))
        }
    }
}

pub async fn stop_service() -> Json<ApiResponse<()>> {
    stop_sing_internal().await;
    success_no_data("sing-box stopped")
}

#[derive(Deserialize)]
pub(crate) struct ConnectivityRequest {
    url: String,
}

pub async fn test_connectivity(
    Json(req): Json<ConnectivityRequest>,
) -> Json<ApiResponse<ConnectivityResult>> {
    let start = Instant::now();
    let result = match CLIENT
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
    use super::get_status;
    use crate::test_support::reset_test_globals;

    #[tokio::test]
    async fn get_status_reports_stopped_when_no_process_exists() {
        reset_test_globals().await;

        let axum::response::Json(response) = get_status().await;

        assert!(response.success);
        assert_eq!(response.message, "stopped");
        let data = response.data.unwrap();
        assert!(!data.running);
        assert!(data.pid.is_none());
        assert!(data.uptime_secs.is_none());
    }
}
