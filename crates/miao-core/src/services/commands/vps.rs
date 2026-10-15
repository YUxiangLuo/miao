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
    deploy_vps_with(state, req, |ip, password| async move {
        provision_vps_node(&ip, &password).await
    })
    .await
}

async fn deploy_vps_with<F: std::future::Future<Output = crate::error::AppResult<String>>>(
    state: Arc<AppState>,
    req: VpsDeployRequest,
    provision: impl FnOnce(String, String) -> F,
) -> CommandResult<VpsDeployResponse> {
    super::ensure_initialized(&state)?;

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

    // Hold a per-address lock across SSH and config commit, never the global
    // config lock during network I/O. Weak entries don't retain idle locks.
    let deployment = {
        let mut deployments = state.vps_deployments.lock().await;
        deployments.retain(|_, lock| lock.strong_count() > 0);
        let entry = deployments.entry(ip.to_string()).or_default();
        match entry.upgrade() {
            Some(lock) => lock,
            None => {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                *entry = Arc::downgrade(&lock);
                lock
            }
        }
    };
    let _deployment = deployment.lock_owned().await;

    // 排队请求在前一次部署落盘后复用节点，不再次改写远端凭据。
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
    let node_json = provision(ip.to_string(), req.password.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    fn request(ip: &str) -> VpsDeployRequest {
        VpsDeployRequest {
            ip: ip.into(),
            password: "test-password".into(),
        }
    }

    #[tokio::test]
    async fn concurrent_deploys_share_one_provision_and_do_not_block_other_hosts() {
        let (_root, state) = crate::test_support::isolated_stopped_state(Default::default());
        let kernel = state.runtime_paths.runtime_dir.join("sing-box");
        std::fs::write(&kernel, "#!/bin/sh\n[ \"$1\" = check ]\n").unwrap();
        std::fs::set_permissions(kernel, std::fs::Permissions::from_mode(0o755)).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (count, started, resume) = (calls.clone(), entered.clone(), release.clone());
        let first = tokio::spawn(deploy_vps_with(
            state.clone(),
            request("203.0.113.10"),
            move |ip, _| async move {
                count.fetch_add(1, Ordering::Relaxed);
                started.notify_one();
                resume.notified().await;
                Ok(serde_json::json!({
                    "type": "hysteria2", "tag": "deployed", "server": ip,
                    "server_port": 543, "password": "first-credentials",
                    "tls": {"enabled": true, "insecure": true}
                })
                .to_string())
            },
        ));
        entered.notified().await;
        assert!(state.config_update.try_lock().is_ok());
        let count = calls.clone();
        let mut second = Box::pin(deploy_vps_with(
            state.clone(),
            request("203.0.113.10"),
            move |_, _| async move {
                count.fetch_add(1, Ordering::Relaxed);
                std::future::pending::<crate::error::AppResult<String>>().await
            },
        ));
        assert!(futures::poll!(&mut second).is_pending());
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "second request must not overwrite remote credentials"
        );

        // A separate VPS can reach SSH immediately, even while the first waits.
        let other = deploy_vps_with(state.clone(), request("203.0.113.11"), |_, _| async {
            Err(crate::error::AppError::message("other host reached"))
        });
        let error = tokio::time::timeout(std::time::Duration::from_secs(1), other)
            .await
            .unwrap()
            .err()
            .unwrap();
        assert!(error.to_string().contains("other host reached"));
        release.notify_one();
        let first = first.await.unwrap().unwrap().data.unwrap();
        let second = second.await.unwrap().data.unwrap();
        assert_eq!(first.tag, second.tag);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let config = state.config.read().await;
        assert_eq!(config.nodes.len(), 1);
        let node: serde_json::Value = serde_json::from_str(&config.nodes[0]).unwrap();
        assert_eq!(node["password"], "first-credentials");
    }
}
