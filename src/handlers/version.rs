use axum::{extract::State, response::Json};
use std::sync::Arc;

use crate::models::{ApiResponse, VersionInfo};
use crate::responses::{error, success, success_no_data};
use crate::services::version::{get_version_info, upgrade_binary};
use crate::state::AppState;

pub async fn get_version(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<VersionInfo>> {
    success("Version info", get_version_info(&state.http_client).await)
}

pub async fn upgrade(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<String>> {
    match upgrade_binary(&state).await {
        Ok(message) if message == "Already up to date" => success_no_data(message),
        Ok(version) => success("Upgrade complete, restarting...", version),
        Err(e) => error(e.to_string()),
    }
}
