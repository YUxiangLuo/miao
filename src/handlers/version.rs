use axum::{extract::State, response::Json};
use std::sync::Arc;

use crate::models::{ApiResponse, VersionInfo};
use crate::responses::{error, success, success_no_data};
use crate::services::version::{get_version_info, upgrade_binary};
use crate::state::AppState;

pub async fn get_version(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<VersionInfo>> {
    success("Version info", get_version_info(&state).await)
}

pub async fn upgrade(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<String>> {
    // 检查是否已有升级正在进行
    if state.upgrading.load(std::sync::atomic::Ordering::SeqCst) {
        return Json(ApiResponse {
            success: false,
            message: "Upgrade already in progress".to_string(),
            data: None,
        });
    }
    
    match upgrade_binary(&state).await {
        Ok(message) if message == "Already up to date" => {
            success_no_data(message)
        }
        Ok(version) => success(
            format!("Upgrade to {} complete, restarting...", version), 
            version
        ),
        Err(e) => {
            let error_msg = e.to_string();
            // 检查是否包含回退相关的信息
            let final_msg = if error_msg.contains("restored") || error_msg.contains("rollback") {
                format!("{} (System rolled back to previous version)", error_msg)
            } else {
                error_msg
            };
            error(final_msg)
        }
    }
}
