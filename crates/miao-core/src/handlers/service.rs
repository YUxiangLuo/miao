use crate::models::{
    ApiResponse, ConnectivityResult, MaxMultiplierRequest, NodeSelectRequest, RouteModeRequest,
    ScheduledRefreshRequest, ScheduledRefreshStatus, StatusData,
};
use crate::responses::{command_reply, command_result, HandlerResult};
use crate::services::commands;
use crate::state::AppState;
use axum::{extract::State, response::Json};
pub(crate) use commands::service::ConnectivityRequest;
use std::sync::Arc;

pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<ApiResponse<StatusData>> {
    command_reply(commands::service::get_status(state).await)
}

pub async fn start_service(State(state): State<Arc<AppState>>) -> HandlerResult {
    command_result(commands::service::start_service(state).await)
}

pub async fn stop_service(State(state): State<Arc<AppState>>) -> HandlerResult {
    command_result(commands::service::stop_service(state).await)
}

pub async fn set_route_mode(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RouteModeRequest>,
) -> HandlerResult {
    command_result(commands::service::set_route_mode(state, req).await)
}

pub async fn set_max_multiplier(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MaxMultiplierRequest>,
) -> HandlerResult {
    command_result(commands::service::set_max_multiplier(state, req).await)
}

pub async fn set_node_select(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NodeSelectRequest>,
) -> HandlerResult {
    command_result(commands::service::set_node_select(state, req).await)
}

pub async fn get_scheduled_refresh(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<ScheduledRefreshStatus>> {
    command_reply(commands::settings::get_scheduled_refresh(state).await)
}

pub async fn set_scheduled_refresh(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScheduledRefreshRequest>,
) -> HandlerResult {
    // 与其它设置类端点一致：REST 只回消息，完整状态由 GET 读取（MCP 仍返回 data）。
    let result = commands::settings::set_scheduled_refresh(state, req)
        .await
        .map(|reply| crate::services::commands::CommandReply::<()> {
            message: reply.message,
            data: None,
        });
    command_result(result)
}

pub async fn test_connectivity(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ConnectivityRequest>,
) -> Json<ApiResponse<ConnectivityResult>> {
    command_reply(commands::service::test_connectivity(state, req).await)
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

        assert!(!state.lifecycle.snapshot().should_run);
    }

    #[tokio::test]
    async fn start_rejects_an_empty_configuration() {
        let state = app_state(Config::default());
        state.initializing.store(false, Ordering::Relaxed);
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

        let status = match start_service(State(state.clone())).await {
            Ok(_) => panic!("empty configuration unexpectedly started"),
            Err((status, _)) => status,
        };

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!state.lifecycle.snapshot().should_run);
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
