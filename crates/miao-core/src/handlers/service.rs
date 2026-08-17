use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Instant,
};
use tokio::time::Duration;

use crate::error::AppError;
use crate::models::{
    ApiResponse, ConnectivityResult, NodeSelect, NodeSelectRequest, RouteModeRequest, StatusData,
};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::{
    config::apply_config_change,
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
    let (route_mode, adblock, mcp, node_select) = {
        let config = state.config.read().await;
        (
            config.route_mode,
            config.adblock,
            config.mcp,
            config.node_select,
        )
    };

    success(
        if running { "running" } else { "stopped" },
        StatusData {
            running,
            initializing,
            route_mode,
            node_select,
            adblock,
            pid,
            uptime_secs,
            warning,
            vps_supported: crate::platform::vps_supported(),
            platform: if cfg!(windows) { "windows" } else { "linux" },
            mcp,
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
    let old_config = state.config.read().await.clone();

    if old_config.route_mode == req.route_mode {
        return Ok(success_no_data("Route mode unchanged"));
    }

    let mut new_config = old_config.clone();
    new_config.route_mode = req.route_mode;

    // route_mode 是易变层字段：走普通配置事务（纯本地语义变更，快照零网络重建），
    // 分层落盘只写 volatile.yaml
    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success_no_data("Route mode updated")),
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

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    if old_config.node_select == node_select {
        return Ok(success_no_data("Node select unchanged"));
    }

    let mut new_config = old_config.clone();
    new_config.node_select = node_select;

    match crate::services::config::apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => {
            let effective = state.config.read().await.node_select;
            if !node_select.is_manual() && effective.is_manual() {
                Ok(success_no_data("该地区没有可用节点，已切回手动选择"))
            } else {
                Ok(success_no_data("Node select updated"))
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
            adblock: false,
            mcp: false,
            node_select: Default::default(),
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
    async fn get_status_reports_route_mode_from_config() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: RouteMode::Global,
            adblock: false,
            mcp: false,
            node_select: Default::default(),
        });

        let axum::response::Json(response) = get_status(State(state)).await;

        let data = response.data.unwrap();
        assert_eq!(data.route_mode, RouteMode::Global);
    }
}
