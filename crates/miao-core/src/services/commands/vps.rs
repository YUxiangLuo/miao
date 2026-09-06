use std::sync::Arc;

use super::{command_error, success, CommandErrorKind, CommandResult};
use crate::models::{VpsDeployRequest, VpsDeployResponse};
use crate::services::config::ConfigEdit;
use crate::services::vps::{node_tag_for_vps, provision_vps_node};
use crate::state::AppState;
use crate::validation::Validator;

pub async fn deploy_vps(
    state: Arc<AppState>,
    req: VpsDeployRequest,
) -> CommandResult<VpsDeployResponse> {
    super::ensure_initialized(&state)?;

    if !crate::platform::vps_supported() {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "当前平台不支持 VPS 一键部署",
        ));
    }

    let ip = req.ip.trim();
    Validator::server_address(ip).map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;
    if req.password.is_empty() {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "root 密码不能为空",
        ));
    }
    if req.password.len() > 256 {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "root 密码过长",
        ));
    }

    // 该 VPS 的节点已存在时不重复部署（部署前的快速检查，不持锁）
    {
        let config = state.config.read().await;
        if let Some(tag) = node_tag_for_vps(&config, ip) {
            return Ok(success(
                format!("该 VPS 的节点已存在: {tag}"),
                VpsDeployResponse { tag },
            ));
        }
    }

    // SSH 供给可能耗时数分钟：不持 config_update 锁，避免阻塞所有配置变更。
    // 供给只产出节点 JSON、不触碰配置；节点在下面的锁内随事务提交落盘。
    let node_json = provision_vps_node(ip, &req.password)
        .await
        .map_err(|e| command_error(CommandErrorKind::Upstream, format!("VPS 部署失败: {e}")))?;

    let mut edit = ConfigEdit::begin(&state).await;

    // 供给期间其他变更可能已添加同一 VPS 的节点
    if let Some(tag) = node_tag_for_vps(edit.original(), ip) {
        return Ok(success(
            format!("该 VPS 的节点已存在: {tag}"),
            VpsDeployResponse { tag },
        ));
    }

    edit.candidate.nodes.push(node_json);

    let tag = node_tag_for_vps(&edit.candidate, ip)
        .ok_or_else(|| command_error(CommandErrorKind::Internal, "部署完成但未找到节点"))?;
    edit.commit()
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;

    Ok(success(
        format!("VPS 节点已添加: {tag}"),
        VpsDeployResponse { tag },
    ))
}
