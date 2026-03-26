use axum::response::Json;

use crate::models::{ApiResponse, LastProxy};
use crate::responses::{error, success_no_data};
use crate::services::proxy::save_last_proxy;

pub async fn set_last_proxy(Json(req): Json<LastProxy>) -> Json<ApiResponse<()>> {
    match save_last_proxy(&req).await {
        Ok(_) => success_no_data("Last proxy saved"),
        Err(e) => error(format!("Failed to save: {}", e)),
    }
}
