use axum::{
    extract::{Query, State},
    response::Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::{ApiResponse, VersionInfo};
use crate::responses::success;
#[cfg(not(windows))]
use crate::responses::{error, success_no_data};
use crate::services::version::get_version_info_with_refresh;
#[cfg(not(windows))]
use crate::services::version::upgrade_binary;
use crate::state::AppState;

#[derive(Default, Deserialize)]
pub struct VersionQuery {
    #[serde(default)]
    pub refresh: Option<String>,
}

pub async fn get_version(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VersionQuery>,
) -> Json<ApiResponse<VersionInfo>> {
    let force_refresh = matches!(query.refresh.as_deref(), Some("1" | "true"));
    success(
        "Version info",
        get_version_info_with_refresh(&state, force_refresh).await,
    )
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
