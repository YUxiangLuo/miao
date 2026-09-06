use super::{command_error, success_no_data, CommandErrorKind, CommandResult};
use crate::models::McpRequest;
use crate::services::config::save_stable_fields;
use crate::state::AppState;
use std::sync::{atomic::Ordering, Arc};

pub async fn set_mcp(state: Arc<AppState>, req: McpRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    if old_config.mcp == req.enabled {
        return Ok(success_no_data("MCP setting unchanged"));
    }

    let mut new_config = old_config.clone();
    new_config.mcp = req.enabled;
    save_stable_fields(&state, &new_config)
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    *state.config.write().await = new_config;
    state.data_revision.fetch_add(1, Ordering::Relaxed);

    Ok(success_no_data(if req.enabled {
        "MCP enabled"
    } else {
        "MCP disabled"
    }))
}
