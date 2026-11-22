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
