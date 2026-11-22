use std::sync::Arc;

use axum::{extract::State, response::Json};

use crate::models::{
    AccessPortStatus, ApiResponse, DeleteAccessPortRequest, SaveAccessPortRequest,
};
use crate::responses::{command_reply, command_result, HandlerResult};
use crate::services::commands;
use crate::state::AppState;

pub async fn get_access_ports(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<AccessPortStatus>>> {
    command_reply(commands::access_ports::get_access_ports(state).await)
}

pub async fn save_access_port(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SaveAccessPortRequest>,
) -> HandlerResult<AccessPortStatus> {
    command_result(commands::access_ports::save_access_port(state, req).await)
}

pub async fn delete_access_port(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteAccessPortRequest>,
) -> HandlerResult {
    command_result(commands::access_ports::delete_access_port(state, req).await)
}
