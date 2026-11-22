use axum::{extract::State, response::Json};
use std::sync::Arc;

use crate::models::{ApiResponse, VersionInfo};
use crate::responses::success;
#[cfg(not(windows))]
use crate::responses::{error, success_no_data};
use crate::services::version::get_version_info;
#[cfg(not(windows))]
use crate::services::version::upgrade_binary;
use crate::state::AppState;

pub async fn get_version(State(state): State<Arc<AppState>>) -> Json<ApiResponse<VersionInfo>> {
    success("Version info", get_version_info(&state).await)
}

#[cfg(not(windows))]
pub async fn upgrade(State(state): State<Arc<AppState>>) -> Json<ApiResponse<String>> {
    match upgrade_binary(&state).await {
        Ok(message) if message == "Already up to date" => success_no_data(message),
        Ok(version) => success(
            format!("Upgrade to {} complete, restarting...", version),
            version,
        ),
        Err(e) => error(e.to_string()),
    }
}
