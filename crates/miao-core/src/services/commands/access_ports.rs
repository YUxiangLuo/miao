use std::collections::HashSet;
use std::sync::Arc;

use super::{
    command_error, success, success_no_data, CommandErrorKind, CommandReply, CommandResult,
};
use crate::models::{
    AccessPortConfig, AccessPortStatus, DeleteAccessPortRequest, SaveAccessPortRequest,
    DEFAULT_PORT,
};
use crate::services::config::{known_rule_targets, ConfigEdit};
use crate::state::AppState;
use crate::validation::Validator;

const CLASH_INTERNAL_PORT: u16 = 6262;

fn validate_access_port_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 48 {
        return Err("专属端口标识长度需在 1-48 个字符之间".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("专属端口标识只能包含字母、数字、下划线和连字符".to_string());
    }
    Ok(trimmed.to_string())
}

fn generate_access_port_id(port: u16, existing: &[AccessPortConfig]) -> String {
    let base = format!("ap-{port}");
    if existing.iter().all(|item| item.id != base) {
        return base;
    }
    for index in 2.. {
        let candidate = format!("{base}-{index}");
        if existing.iter().all(|item| item.id != candidate) {
            return candidate;
        }
    }
    unreachable!("unbounded id search must find unused id")
}

fn to_access_port_status(
    config: &AccessPortConfig,
    blocked_ids: &HashSet<&str>,
) -> AccessPortStatus {
    let username = config.username.clone().filter(|s| !s.is_empty());
    let password = config.password.clone().filter(|s| !s.is_empty());
    let has_auth = username.is_some() && password.is_some();
    AccessPortStatus {
        id: config.id.clone(),
        name: config.name.clone(),
        port: config.port,
        outbound: config.outbound.clone(),
        username,
        password,
        has_auth,
        blocked: blocked_ids.contains(config.id.as_str()),
    }
}

pub async fn get_access_ports(state: Arc<AppState>) -> CommandReply<Vec<AccessPortStatus>> {
    let access_ports = state.config.read().await.access_ports.clone();
    let blocked = state.blocked_access_ports.lock().await;
    let blocked_ids: HashSet<&str> = blocked.iter().map(|item| item.id.as_str()).collect();

    let items = access_ports
        .iter()
        .map(|item| to_access_port_status(item, &blocked_ids))
        .collect();
    success("Access ports loaded", items)
}

pub async fn save_access_port(
    state: Arc<AppState>,
    req: SaveAccessPortRequest,
) -> CommandResult<AccessPortStatus> {
    super::ensure_initialized(&state)?;

    let name = req.name.trim();
    if name.is_empty() {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "店铺/应用专属入口名称不能为空",
        ));
    }
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "专属入口名称长度需在 1-64 个字符之间且不含控制字符",
        ));
    }
    if req.port < 1024 {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "局域网监听端口需在 1024-65535 之间（推荐使用 20101-29999）",
        ));
    }
    if req.port == CLASH_INTERNAL_PORT {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "端口 6262 为网关内部控制端口，请更换其他端口",
        ));
    }

    let raw_username = req.username.as_deref().unwrap_or_default();
    let raw_password = req.password.as_deref().unwrap_or_default();
    Validator::socks5_auth(raw_username, raw_password)
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;

    let outbound = req.outbound.trim();
    if outbound.is_empty() || outbound == "reject" {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "请选择专属端口绑定的出口专线",
        ));
    }

    let mut edit = ConfigEdit::begin(&state).await;

    let panel_port = edit.original().port.unwrap_or(DEFAULT_PORT);
    if req.port == panel_port {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            format!("端口 {} 与当前控制台端口冲突，请更换其他端口", panel_port),
        ));
    }

    if outbound != "proxy" && outbound != "direct" {
        let known = known_rule_targets(edit.original(), &state).await;
        if !known.iter().any(|existing| existing == outbound) {
            return Err(command_error(
                CommandErrorKind::InvalidInput,
                format!("目标专线节点 '{}' 不存在", outbound),
            ));
        }
    }

    let id = match req.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(explicit_id) => validate_access_port_id(explicit_id)
            .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?,
        None => generate_access_port_id(req.port, &edit.candidate.access_ports),
    };

    if let Some(conflict) = edit
        .candidate
        .access_ports
        .iter()
        .find(|item| item.port == req.port && item.id != id)
    {
        return Err(command_error(
            CommandErrorKind::Conflict,
            format!(
                "局域网端口 {} 已被专属入口「{}」占用，请更换端口",
                req.port, conflict.name
            ),
        ));
    }

    let next_entry = AccessPortConfig {
        id: id.clone(),
        name: name.to_string(),
        port: req.port,
        outbound: outbound.to_string(),
        username: (!raw_username.is_empty()).then(|| raw_username.to_string()),
        password: (!raw_password.is_empty()).then(|| raw_password.to_string()),
    };

    if let Some(existing) = edit
        .candidate
        .access_ports
        .iter_mut()
        .find(|item| item.id == id)
    {
        *existing = next_entry.clone();
    } else {
        edit.candidate.access_ports.push(next_entry.clone());
    }

    if edit.candidate.access_ports != edit.original().access_ports {
        edit.commit()
            .await
            .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    } else {
        drop(edit);
    }

    let blocked = state.blocked_access_ports.lock().await;
    let blocked_ids: HashSet<&str> = blocked.iter().map(|item| item.id.as_str()).collect();
    Ok(success(
        "Access port saved",
        to_access_port_status(&next_entry, &blocked_ids),
    ))
}

pub async fn delete_access_port(
    state: Arc<AppState>,
    req: DeleteAccessPortRequest,
) -> CommandResult {
    super::ensure_initialized(&state)?;
    let id = req.id.trim();
    if id.is_empty() {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "专属端口标识不能为空",
        ));
    }

    let mut edit = ConfigEdit::begin(&state).await;
    let before_len = edit.candidate.access_ports.len();
    edit.candidate.access_ports.retain(|item| item.id != id);
    if edit.candidate.access_ports.len() == before_len {
        return Err(command_error(CommandErrorKind::NotFound, "专属端口不存在"));
    }

    edit.commit()
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    Ok(success_no_data("Access port deleted"))
}

#[cfg(test)]
mod tests {
    use super::{delete_access_port, get_access_ports, save_access_port};
    use crate::models::{Config, DeleteAccessPortRequest, SaveAccessPortRequest};
    use crate::services::commands::CommandErrorKind;
    use crate::test_support::{install_fake_kernel, isolated_stopped_state};

    #[tokio::test]
    async fn save_list_and_delete_access_ports_with_conflict_validation() {
        let (_root, state) = isolated_stopped_state(Config {
            port: Some(6161),
            nodes: vec![
                r#"{"type":"socks","tag":"us-isp-1","server":"198.51.100.1","server_port":1080,"version":"5"}"#.to_string(),
            ],
            ..Default::default()
        });
        install_fake_kernel(&state);

        let created = save_access_port(
            state.clone(),
            SaveAccessPortRequest {
                id: None,
                name: "Amazon 美区一店".to_string(),
                port: 20101,
                outbound: "us-isp-1".to_string(),
                username: Some("amz1".to_string()),
                password: Some("pass1234".to_string()),
            },
        )
        .await
        .unwrap()
        .data
        .unwrap();
        assert_eq!(created.id, "ap-20101");
        assert!(created.has_auth);
        assert!(!created.blocked);

        // Panel port collision rejected
        let err = save_access_port(
            state.clone(),
            SaveAccessPortRequest {
                id: None,
                name: "Bad Port".to_string(),
                port: 6161,
                outbound: "us-isp-1".to_string(),
                username: None,
                password: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(err.kind, CommandErrorKind::InvalidInput);

        // Duplicate port on another entry rejected with Conflict
        let dup_err = save_access_port(
            state.clone(),
            SaveAccessPortRequest {
                id: None,
                name: "Amazon 美区二店".to_string(),
                port: 20101,
                outbound: "us-isp-1".to_string(),
                username: None,
                password: None,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(dup_err.kind, CommandErrorKind::Conflict);

        // Updating same id on same port succeeds
        let updated = save_access_port(
            state.clone(),
            SaveAccessPortRequest {
                id: Some("ap-20101".to_string()),
                name: "Amazon 美区旗舰店".to_string(),
                port: 20101,
                outbound: "us-isp-1".to_string(),
                username: None,
                password: None,
            },
        )
        .await
        .unwrap()
        .data
        .unwrap();
        assert_eq!(updated.name, "Amazon 美区旗舰店");
        assert!(!updated.has_auth);

        let listed = get_access_ports(state.clone()).await.data.unwrap();
        assert_eq!(listed.len(), 1);

        delete_access_port(
            state.clone(),
            DeleteAccessPortRequest {
                id: "ap-20101".to_string(),
            },
        )
        .await
        .unwrap();
        assert!(get_access_ports(state).await.data.unwrap().is_empty());
    }
}
