use axum::{extract::State, response::Json};
use std::sync::Arc;

use crate::models::{ApiResponse, LastProxy};
use crate::responses::{error, success_no_data};
use crate::services::proxy::save_last_proxy;
use crate::state::AppState;

pub async fn set_last_proxy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LastProxy>,
) -> Json<ApiResponse<()>> {
    match save_last_proxy(&state, &req).await {
        Ok(_) => success_no_data("Last proxy saved"),
        Err(e) => error(format!("Failed to save: {}", e)),
    }
}
