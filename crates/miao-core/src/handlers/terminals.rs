use std::sync::Arc;

use axum::{extract::State, response::Json};

use crate::models::{
    ApiResponse, AssignTerminalRequest, DeleteTerminalRequest, TerminalSummary,
    UpsertTerminalRequest,
};
use crate::responses::{command_reply, command_result, HandlerResult};
use crate::services::commands;
use crate::state::AppState;

pub async fn get_terminals(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<TerminalSummary>>> {
    command_reply(commands::terminals::get_terminals(state).await)
}

pub async fn upsert_terminal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpsertTerminalRequest>,
) -> HandlerResult<TerminalSummary> {
    command_result(commands::terminals::upsert_terminal(state, req).await)
}

pub async fn delete_terminal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteTerminalRequest>,
) -> HandlerResult {
    command_result(commands::terminals::delete_terminal(state, req).await)
}

pub async fn assign_terminal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AssignTerminalRequest>,
) -> HandlerResult {
    command_result(commands::terminals::assign_terminal(state, req).await)
}
