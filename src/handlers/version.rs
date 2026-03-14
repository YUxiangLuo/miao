use axum::response::Json;

use crate::models::{ApiResponse, VersionInfo};
use crate::responses::{error, success, success_no_data};
use crate::services::version::{get_version_info, upgrade_binary};

pub async fn get_version() -> Json<ApiResponse<VersionInfo>> {
    success("Version info", get_version_info().await)
}

pub async fn upgrade() -> Json<ApiResponse<String>> {
    match upgrade_binary().await {
        Ok(message) if message == "Already up to date" => success_no_data(message),
        Ok(version) => success("Upgrade complete, restarting...", version),
        Err(e) => error(e.to_string()),
    }
}
