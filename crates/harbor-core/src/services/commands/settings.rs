use super::{
    command_error, success, success_no_data, CommandErrorKind, CommandReply, CommandResult,
};
use crate::models::{
    McpRequest, ScheduledRefresh, ScheduledRefreshRequest, ScheduledRefreshStatus,
};
use crate::services::config::{save_scheduled_refresh, save_stable_fields};
use crate::services::schedule;
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

/// 定时刷新状态投影：纯读，任何时刻都可调用（初始化中也可渲染面板）。
pub async fn get_scheduled_refresh(state: Arc<AppState>) -> CommandReply<ScheduledRefreshStatus> {
    let schedule = state.stable_config.read().await.scheduled_refresh.clone();
    success("Scheduled refresh loaded", schedule::status(&schedule))
}

/// 保存定时刷新设置（稳定层 config.yaml，热生效，不重启 sing-box）。
/// 时刻按系统本地时区解释；启用时要求至少一个有效时刻。
pub async fn set_scheduled_refresh(
    state: Arc<AppState>,
    req: ScheduledRefreshRequest,
) -> CommandResult<ScheduledRefreshStatus> {
    super::ensure_initialized(&state)?;

    let times = schedule::normalize_times(&req.times)
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;
    if req.enabled && times.is_empty() {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "启用定时刷新时至少需要一个时间点",
        ));
    }
    let next = ScheduledRefresh {
        enabled: req.enabled,
        times,
    };

    let _config_update = state.config_update.lock().await;
    if state.stable_config.read().await.scheduled_refresh == next {
        return Ok(success(
            "Scheduled refresh unchanged",
            schedule::status(&next),
        ));
    }

    save_scheduled_refresh(&state, next.clone())
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    // 唤醒调度循环重算下次执行时刻（含关闭与时刻调整）。
    state.scheduled_refresh_wake.notify_waiters();
    state.data_revision.fetch_add(1, Ordering::Relaxed);

    Ok(success(
        if next.enabled {
            "Scheduled refresh enabled"
        } else {
            "Scheduled refresh disabled"
        },
        schedule::status(&next),
    ))
}
