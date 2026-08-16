use axum::{
    body::Bytes, extract::State, http::StatusCode, response::IntoResponse, response::Json,
    response::Response,
};
use std::sync::{atomic::Ordering, Arc};

use crate::models::McpRequest;
use crate::responses::{status_error, success_no_data, HandlerResult};
use crate::services::config::save_config_to;
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

/// POST /api/mcp — 面板里的 MCP 开关。
/// MCP 门控只读内存配置，与内核无关：持久化 + 更新内存即可，
/// 不走 apply_config_change，不重启 sing-box。
pub async fn set_mcp(
    State(state): State<Arc<AppState>>,
    Json(req): Json<McpRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    if old_config.mcp == req.enabled {
        return Ok(success_no_data("MCP setting unchanged"));
    }

    let mut new_config = old_config.clone();
    new_config.mcp = req.enabled;
    save_config_to(&state.config_path, &new_config)
        .await
        .map_err(|e| status_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    *state.config.write().await = new_config;

    Ok(success_no_data(if req.enabled {
        "MCP enabled"
    } else {
        "MCP disabled"
    }))
}

#[cfg(test)]
mod tests {
    use super::set_mcp;
    use crate::models::{Config, McpRequest};
    use crate::state::AppState;
    use axum::{extract::State, http::StatusCode, Json};
    use std::sync::Arc;

    fn temp_config_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mcp-toggle-test-{tag}-{}-{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ))
    }

    #[tokio::test]
    async fn set_mcp_is_idempotent_when_state_matches() {
        let state = Arc::new(AppState::new(Config::default()).unwrap());
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let result = set_mcp(State(state), Json(McpRequest { enabled: false })).await;
        let Json(response) = result.ok().expect("idempotent toggle should succeed");
        assert_eq!(response.message, "MCP setting unchanged");
    }

    #[tokio::test]
    async fn set_mcp_persists_without_touching_the_kernel() {
        let config_path = temp_config_path("persist");
        let state =
            Arc::new(AppState::with_config_path(Config::default(), config_path.clone()).unwrap());
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let result = set_mcp(State(state.clone()), Json(McpRequest { enabled: true })).await;
        let Json(response) = result.ok().expect("toggle should succeed");
        assert_eq!(response.message, "MCP enabled");

        // 内存态已翻转
        assert!(state.config.read().await.mcp);
        // 配置已落盘（轻量路径：只写 yaml，不生成内核配置、不起 sing-box）
        let saved = tokio::fs::read_to_string(&config_path).await.unwrap();
        assert!(saved.contains("mcp: true"));
        let _ = tokio::fs::remove_file(&config_path).await;
    }

    #[tokio::test]
    async fn set_mcp_rejects_during_initialization() {
        let state = Arc::new(AppState::new(Config::default()).unwrap());
        let result = set_mcp(State(state), Json(McpRequest { enabled: true })).await;
        match result {
            Ok(_) => panic!("should conflict during initialization"),
            Err((status, _)) => assert_eq!(status, StatusCode::CONFLICT),
        }
    }
}
