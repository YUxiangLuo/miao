use crate::models::{VpsDeployRequest, VpsDeployResponse};
use crate::responses::{command_result, HandlerResult};
use crate::services::commands;
use crate::state::AppState;
use axum::{extract::State, response::Json};
use std::sync::Arc;

pub async fn deploy_vps(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VpsDeployRequest>,
) -> HandlerResult<VpsDeployResponse> {
    command_result(commands::vps::deploy_vps(state, req).await)
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, Json};

    use super::deploy_vps;
    use crate::models::{Config, VpsDeployRequest};
    use crate::test_support::app_state;

    #[tokio::test]
    async fn deploy_vps_is_rejected_when_platform_cannot_run_askpass() {
        if crate::platform::vps_supported() {
            return;
        }

        let state = app_state(Config::default());
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let status = match deploy_vps(
            State(state),
            Json(VpsDeployRequest {
                ip: "203.0.113.10".into(),
                password: "secret".into(),
            }),
        )
        .await
        {
            Ok(_) => panic!("windows vps deploy unexpectedly succeeded"),
            Err((status, _)) => status,
        };

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
