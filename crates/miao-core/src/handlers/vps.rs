use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::{atomic::Ordering, Arc};

use crate::models::{VpsDeployRequest, VpsDeployResponse};
use crate::responses::{status_error, success, HandlerResult};
use crate::services::config::apply_config_change;
use crate::services::vps::{node_tag_for_vps, provision_vps_node};
use crate::state::AppState;
use crate::validation::Validator;

pub async fn deploy_vps(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VpsDeployRequest>,
) -> HandlerResult<VpsDeployResponse> {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    if !crate::platform::vps_supported() {
        return Err(status_error(
            StatusCode::BAD_REQUEST,
            "当前平台不支持 VPS 一键部署",
        ));
    }

    let ip = req.ip.trim();
    Validator::server_address(ip).map_err(|e| status_error(StatusCode::BAD_REQUEST, e))?;
    if req.password.is_empty() {
        return Err(status_error(StatusCode::BAD_REQUEST, "root 密码不能为空"));
    }
    if req.password.len() > 256 {
        return Err(status_error(StatusCode::BAD_REQUEST, "root 密码过长"));
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
        .map_err(|e| status_error(StatusCode::BAD_GATEWAY, format!("VPS 部署失败: {e}")))?;

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();

    // 供给期间其他变更可能已添加同一 VPS 的节点
    if let Some(tag) = node_tag_for_vps(&old_config, ip) {
        return Ok(success(
            format!("该 VPS 的节点已存在: {tag}"),
            VpsDeployResponse { tag },
        ));
    }

    let mut new_config = old_config.clone();
    new_config.nodes.push(node_json);

    apply_config_change(&state, &old_config, &new_config)
        .await
        .map_err(|e| status_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let tag = node_tag_for_vps(&new_config, ip)
        .ok_or_else(|| status_error(StatusCode::INTERNAL_SERVER_ERROR, "部署完成但未找到节点"))?;

    Ok(success(
        format!("VPS 节点已添加: {tag}"),
        VpsDeployResponse { tag },
    ))
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, Json};

    use super::deploy_vps;
    use crate::models::{Config, VpsDeployRequest};
    use crate::test_support::app_state;

    #[tokio::test]
    async fn deploy_vps_is_rejected_when_platform_cannot_run_askpass() {
        if crate::platform::vps_supported() {
            return;
        }

        let state = app_state(Config::default());
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let status = match deploy_vps(
            State(state),
            Json(VpsDeployRequest {
                ip: "203.0.113.10".into(),
                password: "secret".into(),
            }),
        )
        .await
        {
            Ok(_) => panic!("windows vps deploy unexpectedly succeeded"),
            Err((status, _)) => status,
        };

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
