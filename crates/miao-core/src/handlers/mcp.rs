use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, response::Response,
};
use std::sync::Arc;

use crate::state::AppState;

/// POST /mcp — MCP（Model Context Protocol）JSON-RPC 端点。
/// 默认关闭（config `mcp: true` 开启）；关闭时表现为 404，不暴露端点存在。
pub async fn handle_mcp(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    if !state.config.read().await.mcp {
        return StatusCode::NOT_FOUND.into_response();
    }

    match crate::services::mcp::handle(&state, &body).await {
        Some(payload) => (
            [(
                "MCP-Protocol-Version",
                crate::services::mcp::MCP_PROTOCOL_VERSION,
            )],
            axum::Json(payload),
        )
            .into_response(),
        // 通知类消息无需应答
        None => StatusCode::ACCEPTED.into_response(),
    }
}
