use std::io::Read;
use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, Hysteria2, Hysteria2Obfs, Tls};
use crate::services::node_parser::parse_node_json;
use crate::validation::Validator;

const HYSTERIA_PORT: u16 = 543;
const HYSTERIA_OBFS_TYPE: &str = "gecko";
const SSH_CONNECT_TIMEOUT_SECS: &str = "10";
const SSH_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const SSH_PROVISION_TIMEOUT: Duration = Duration::from_secs(300);

fn set_file_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// SSH_ASKPASS 临时文件:密码写 600 权限文件,askpass 脚本(700)cat 它。
/// 密码不出现在进程 argv;文件随作用域删除,不落配置。
/// (OpenSSH >= 8.4 的 SSH_ASKPASS_REQUIRE=force,无需 sshpass 依赖)
struct AskpassFiles {
    script_path: std::path::PathBuf,
    password_path: std::path::PathBuf,
}

impl AskpassFiles {
    fn new(password: &str) -> AppResult<Self> {
        let id = random_password()?;
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("miao-askpass-{id}.sh"));
        let password_path = temp_dir.join(format!("miao-askpass-{id}.pw"));
        let cleanup_script = script_path.clone();
        let cleanup_password = password_path.clone();

        let result = (|| -> AppResult<Self> {
            std::fs::write(&password_path, password)
                .map_err(|e| AppError::context("Failed to write askpass password file", e))?;
            set_file_mode(&password_path, 0o600)
                .map_err(|e| AppError::context("Failed to secure askpass password file", e))?;
            std::fs::write(
                &script_path,
                format!("#!/bin/sh\ncat \"{}\"\n", password_path.display()),
            )
            .map_err(|e| AppError::context("Failed to write askpass script", e))?;
            set_file_mode(&script_path, 0o700)
                .map_err(|e| AppError::context("Failed to secure askpass script", e))?;
            Ok(Self {
                script_path,
                password_path,
            })
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&cleanup_script);
            let _ = std::fs::remove_file(&cleanup_password);
        }
        result
    }
}

impl Drop for AskpassFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.script_path);
        let _ = std::fs::remove_file(&self.password_path);
    }
}

/// 把敏感参数作为 shell 变量赋值前缀拼进 stdin 脚本,而不是放在远端进程
/// argv(/proc/<pid>/cmdline 对 VPS 上所有用户可读;stdin 不可见)。
/// 值都是本地生成的 hex 字符串,单引号包裹无注入面。
fn with_shell_vars(script: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(script.len() + 64);
    for (key, value) in vars {
        out.push_str(key);
        out.push_str("='");
        out.push_str(value);
        out.push_str("'\n");
    }
    out.push_str(script);
    out
}

/// 构建 ssh 命令;密码认证通过 env 注入 askpass,密码不进 argv。
/// StrictHostKeyChecking=accept-new 是 TOFU:首次连接信任并记录主机密钥到
/// root 的 known_hosts(install.sh 的 systemd 单元无 ProtectHome,可写),
/// 之后同一主机密钥变更会被拒绝。首连存在 MITM 窗口——
/// 密码认证且没有预共享指纹时的固有取舍。
fn build_ssh_command(
    vps_ip: &str,
    askpass: &AskpassFiles,
    script_args: &[&str],
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("ssh");
    command
        .env("SSH_ASKPASS", &askpass.script_path)
        .env("SSH_ASKPASS_REQUIRE", "force")
        // ssh 只在认定无 tty 且设置了 DISPLAY 时才走 askpass
        .env("DISPLAY", "localhost:0")
        .args([
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            &format!("ConnectTimeout={SSH_CONNECT_TIMEOUT_SECS}"),
        ])
        .arg(format!("root@{vps_ip}"))
        .args(script_args);
    command
}

#[derive(Debug)]
struct HysteriaCredentials {
    password: String,
    obfs_password: String,
}

/// 远程 Hysteria2 部署的状态,由 SSH 探针脚本的退出码区分:
/// - 0:   miao 部署的,凭据可复用
/// - 10:  无 hysteria 配置,可直接全新部署
/// - 30:  存在 hysteria 服务但不是 miao 部署的,需先清理再重新部署
enum RemoteHysteriaState {
    Reusable(HysteriaCredentials),
    NotFound,
    NeedsCleanup,
}

/// 找到 server 与给定 VPS IP 匹配的手动节点 tag
pub fn node_tag_for_vps(config: &Config, vps_ip: &str) -> Option<String> {
    config.nodes.iter().find_map(|node| {
        parse_node_json(node).ok().and_then(|(info, _)| {
            if info.server == vps_ip {
                Some(info.tag)
            } else {
                None
            }
        })
    })
}

/// 通过 root 密码 SSH 在 VPS 上部署(或取回已有)Hysteria2 节点,返回节点 JSON。
/// 密码只用于本次部署,不会被持久化。本函数只做 SSH 供给、不读写配置——
/// 节点由调用方在配置事务内追加并落盘,因此供给期间不需要持有配置锁。
pub async fn provision_vps_node(vps_ip: &str, root_password: &str) -> AppResult<String> {
    let vps_ip = vps_ip.trim().to_string();

    Validator::server_address(&vps_ip)
        .map_err(|e| AppError::message(format!("Invalid VPS address '{}': {}", vps_ip, e)))?;

    let fallback_obfs_password = random_password()?;
    let credentials = match probe_remote_hysteria_credentials(
        &vps_ip,
        &fallback_obfs_password,
        root_password,
    )
    .await
    {
        Ok(RemoteHysteriaState::Reusable(credentials)) => {
            info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, obfs = HYSTERIA_OBFS_TYPE, "Recovered existing VPS Hysteria2 node from remote config");
            credentials
        }
        Ok(RemoteHysteriaState::NotFound) => {
            let credentials = HysteriaCredentials {
                password: random_password()?,
                obfs_password: fallback_obfs_password,
            };
            info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, obfs = HYSTERIA_OBFS_TYPE, "Provisioning Hysteria2 server over SSH");
            provision_remote_hysteria(&vps_ip, &credentials, root_password).await?;
            credentials
        }
        Ok(RemoteHysteriaState::NeedsCleanup) => {
            let credentials = HysteriaCredentials {
                password: random_password()?,
                obfs_password: fallback_obfs_password,
            };
            warn!(vps_ip = %vps_ip, port = HYSTERIA_PORT, obfs = HYSTERIA_OBFS_TYPE, "Existing Hysteria2 service is not deployed by Miao; cleaning up and re-provisioning");
            provision_remote_hysteria(&vps_ip, &credentials, root_password).await?;
            credentials
        }
        // 探针失败(认证失败/不可达)直接报错,不盲目重装
        Err(e) => return Err(e),
    };

    let node_json =
        build_hysteria_node_json(&vps_ip, &credentials.password, &credentials.obfs_password)?;
    info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, "Provisioned VPS Hysteria2 node");
    Ok(node_json)
}

fn build_hysteria_node_json(
    server: &str,
    password: &str,
    obfs_password: &str,
) -> AppResult<String> {
    let node = Hysteria2 {
        outbound_type: "hysteria2".to_string(),
        tag: vps_node_tag(server),
        server: server.to_string(),
        server_port: HYSTERIA_PORT,
        password: password.to_string(),
        up_mbps: None,
        down_mbps: None,
        obfs: Some(Hysteria2Obfs {
            obfs_type: HYSTERIA_OBFS_TYPE.to_string(),
            password: obfs_password.to_string(),
        }),
        tls: Tls {
            enabled: true,
            server_name: None,
            insecure: true,
        },
    };

    serde_json::to_string(&node).map_err(AppError::from)
}

fn vps_node_tag(server: &str) -> String {
    let mut tag = String::from("vps-");
    for ch in server.chars() {
        if ch.is_ascii_alphanumeric() {
            tag.push(ch.to_ascii_lowercase());
        } else if !tag.ends_with('-') {
            tag.push('-');
        }
    }

    while tag.ends_with('-') {
        tag.pop();
    }

    if tag.len() > 64 {
        tag.truncate(64);
        while tag.ends_with('-') {
            tag.pop();
        }
    }

    if tag == "vps" {
        "vps-node".to_string()
    } else {
        tag
    }
}

fn random_password() -> AppResult<String> {
    let mut bytes = [0u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|e| AppError::context("Failed to generate VPS node password", e))?;
    Ok(hex::encode(bytes))
}

async fn probe_remote_hysteria_credentials(
    vps_ip: &str,
    fallback_obfs_password: &str,
    root_password: &str,
) -> AppResult<RemoteHysteriaState> {
    let askpass = AskpassFiles::new(root_password)?;
    let mut child = build_ssh_command(vps_ip, &askpass, &["bash", "-s"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::context("Failed to start ssh for VPS config probe", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        let script = with_shell_vars(
            remote_hysteria_probe_script(),
            &[("FALLBACK_OBFS_PASSWORD", fallback_obfs_password)],
        );
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| AppError::context("Failed to send VPS config probe script over ssh", e))?;
    }

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::message("Failed to capture ssh probe stdout"))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::message("Failed to capture ssh probe stderr"))?;

    let status = match timeout(SSH_PROBE_TIMEOUT, child.wait()).await {
        Ok(result) => {
            result.map_err(|e| AppError::context("Failed to wait for ssh config probe", e))?
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(AppError::message(
                "Timed out while probing VPS Hysteria2 config over ssh",
            ));
        }
    };

    let mut stdout_buf = Vec::new();
    stdout
        .read_to_end(&mut stdout_buf)
        .await
        .map_err(|e| AppError::context("Failed to read ssh probe stdout", e))?;
    let mut stderr_buf = Vec::new();
    stderr
        .read_to_end(&mut stderr_buf)
        .await
        .map_err(|e| AppError::context("Failed to read ssh probe stderr", e))?;

    if status.success() {
        return parse_probe_credentials(&stdout_buf).map(RemoteHysteriaState::Reusable);
    }

    let stderr_text = String::from_utf8_lossy(&stderr_buf);
    match status.code() {
        Some(10) => {
            info!(vps_ip = %vps_ip, "No reusable remote Hysteria2 config found");
            Ok(RemoteHysteriaState::NotFound)
        }
        Some(30) => {
            info!(
                vps_ip = %vps_ip,
                reason = %stderr_text.trim(),
                "Remote Hysteria2 service is not deployed by Miao; it will be cleaned up and re-provisioned"
            );
            Ok(RemoteHysteriaState::NeedsCleanup)
        }
        _ => {
            let message = stderr_text.trim();
            if message.is_empty() {
                Err(AppError::message(format!(
                    "VPS Hysteria2 config probe failed with status {}",
                    status
                )))
            } else {
                Err(AppError::message(format!(
                    "VPS Hysteria2 config probe failed with status {}: {}",
                    status, message
                )))
            }
        }
    }
}

fn parse_probe_credentials(stdout: &[u8]) -> AppResult<HysteriaCredentials> {
    let stdout = String::from_utf8_lossy(stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());

    let password = lines
        .next()
        .ok_or_else(|| AppError::message("Remote Hysteria2 config probe returned no password"))?;
    let obfs_password = lines.next().ok_or_else(|| {
        AppError::message("Remote Hysteria2 config probe returned no obfs password")
    })?;

    if password.len() > 256 {
        return Err(AppError::message(
            "Remote Hysteria2 password is too long to store locally",
        ));
    }
    if obfs_password.len() > 256 {
        return Err(AppError::message(
            "Remote Hysteria2 obfs password is too long to store locally",
        ));
    }

    Ok(HysteriaCredentials {
        password: password.to_string(),
        obfs_password: obfs_password.to_string(),
    })
}

async fn provision_remote_hysteria(
    vps_ip: &str,
    credentials: &HysteriaCredentials,
    root_password: &str,
) -> AppResult<()> {
    let askpass = AskpassFiles::new(root_password)?;
    let mut child = build_ssh_command(vps_ip, &askpass, &["bash", "-s"])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| AppError::context("Failed to start ssh for VPS provisioning", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        let script = with_shell_vars(
            remote_hysteria_script(),
            &[
                ("PASSWORD", &credentials.password),
                ("OBFS_PASSWORD", &credentials.obfs_password),
            ],
        );
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| AppError::context("Failed to send VPS provisioning script over ssh", e))?;
    }

    let status = match timeout(SSH_PROVISION_TIMEOUT, child.wait()).await {
        Ok(result) => {
            result.map_err(|e| AppError::context("Failed to wait for ssh provisioning", e))?
        }
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(AppError::message(
                "Timed out while provisioning VPS over ssh",
            ));
        }
    };

    if !status.success() {
        return Err(AppError::message(format!(
            "VPS provisioning over ssh failed with status {}",
            status
        )));
    }

    Ok(())
}

fn remote_hysteria_probe_script() -> &'static str {
    include_str!("vps/probe.sh")
}

fn remote_hysteria_script() -> &'static str {
    include_str!("vps/provision.sh")
}

#[cfg(test)]
mod tests {
    use super::{
        build_hysteria_node_json, build_ssh_command, node_tag_for_vps, parse_probe_credentials,
        remote_hysteria_probe_script, remote_hysteria_script, vps_node_tag, with_shell_vars,
        AskpassFiles, HYSTERIA_PORT,
    };
    use crate::models::Config;
    use std::ffi::OsStr;

    fn command_has_env(cmd: &std::process::Command, key: &str, expected: &str) -> bool {
        cmd.get_envs()
            .any(|(k, v)| k == OsStr::new(key) && v == Some(OsStr::new(expected)))
    }

    #[test]
    fn password_auth_uses_askpass_and_keeps_password_out_of_argv() {
        let password = "s3cret-root-password";
        let askpass = AskpassFiles::new(password).unwrap();
        let cmd = build_ssh_command("203.0.113.10", &askpass, &["bash", "-s"]);
        let std_cmd = cmd.as_std();

        assert!(command_has_env(std_cmd, "SSH_ASKPASS_REQUIRE", "force"));
        assert!(command_has_env(std_cmd, "DISPLAY", "localhost:0"));
        assert!(std_cmd
            .get_envs()
            .any(|(k, v)| k == "SSH_ASKPASS" && v.is_some()));
        // 密码不出现在进程参数里
        assert!(!std_cmd.get_args().any(|a| a == password));

        // askpass 脚本内容是读取密码文件,而不是内嵌密码
        let script = std::fs::read_to_string(&askpass.script_path).unwrap();
        assert!(script.contains("cat"));
        assert!(!script.contains(password));
    }

    #[test]
    fn node_tag_lookup_matches_by_server_ip() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"manual","server":"203.0.113.10","server_port":543,"password":"secret","tls":{"enabled":true,"insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        };

        assert_eq!(
            node_tag_for_vps(&config, "203.0.113.10"),
            Some("manual".to_string())
        );
        assert_eq!(node_tag_for_vps(&config, "198.51.100.1"), None);
    }

    #[test]
    fn builds_hysteria2_node_for_self_signed_vps() {
        let node = build_hysteria_node_json("203.0.113.10", "password123", "obfs-secret").unwrap();
        let value: serde_json::Value = serde_json::from_str(&node).unwrap();

        assert_eq!(value["type"], "hysteria2");
        assert_eq!(value["tag"], "vps-203-0-113-10");
        assert_eq!(value["server"], "203.0.113.10");
        assert_eq!(value["server_port"], HYSTERIA_PORT);
        assert_eq!(value["password"], "password123");
        assert_eq!(value["obfs"]["type"], "gecko");
        assert_eq!(value["obfs"]["password"], "obfs-secret");
        assert_eq!(value["tls"]["enabled"], true);
        assert_eq!(value["tls"]["insecure"], true);
    }

    #[test]
    fn vps_node_tag_is_stable_and_limited() {
        assert_eq!(vps_node_tag("Example.COM"), "vps-example-com");
        assert!(vps_node_tag(&"a".repeat(100)).len() <= 64);
    }

    #[test]
    fn parse_probe_credentials_uses_first_two_non_empty_lines() {
        let credentials =
            parse_probe_credentials(b"\n  recovered-password  \n  recovered-obfs  \nextra\n")
                .unwrap();

        assert_eq!(credentials.password, "recovered-password");
        assert_eq!(credentials.obfs_password, "recovered-obfs");
    }

    #[test]
    fn parse_probe_credentials_rejects_empty_output() {
        let err = parse_probe_credentials(b"\n  \n").unwrap_err();

        assert!(err.to_string().contains("returned no password"));
    }

    #[test]
    fn parse_probe_credentials_requires_obfs_password() {
        let err = parse_probe_credentials(b"auth-password\n").unwrap_err();

        assert!(err.to_string().contains("returned no obfs password"));
    }

    #[test]
    fn probe_script_checks_for_existing_miao_hysteria_config() {
        let script = remote_hysteria_probe_script();

        assert!(script.contains("/etc/hysteria/config.yaml"));
        assert!(script.contains("listen:[[:space:]]*:543"));
        assert!(script.contains("type: gecko"));
        assert!(script.contains("password: ${GECKO_PASSWORD}"));
        assert!(script.contains("systemctl restart"));
        assert!(script.contains("printf '%s\\n' \"$PASSWORD\""));
        assert!(script.contains("printf '%s\\n' \"$GECKO_PASSWORD\""));
    }

    #[test]
    fn probe_script_marks_non_miao_deployments_for_cleanup() {
        let script = remote_hysteria_probe_script();

        assert!(script.contains("CN[[:space:]]*=[[:space:]]*miao-hysteria"));
        assert!(script.contains("exit 30"));
        assert!(script.contains("not deployed by Miao"));
        assert!(script.contains("has no /etc/hysteria/server.crt"));
    }

    #[test]
    fn provision_script_cleans_up_before_reprovisioning() {
        let script = remote_hysteria_script();

        assert!(script.contains("systemctl stop \"$SERVICE\""));
        assert!(script.contains("systemctl disable \"$SERVICE\""));
        assert!(script.contains("pkill -x hysteria"));
        assert!(script.contains("rm -rf /etc/hysteria"));
        assert!(script.contains("rm -f /usr/local/bin/hysteria"));
        assert!(script.contains("-subj \"/CN=miao-hysteria\""));
    }

    #[test]
    fn provision_script_installs_pinned_verified_hysteria_binary() {
        let script = remote_hysteria_script();

        // 不再 curl|bash 第三方安装脚本;钉版 + 官方校验和验证 + 自带 systemd 单元
        assert!(!script.contains("get.hy2.sh"));
        assert!(script.contains("HYSTERIA_VERSION=\"v"));
        assert!(script.contains("hysteria-linux-${HYSTERIA_ARCH}"));
        assert!(script.contains("hashes.txt"));
        assert!(script.contains("sha256sum"));
        assert!(script.contains("checksum mismatch"));
        assert!(script.contains("/etc/systemd/system/hysteria-server.service"));
        assert!(script.contains("systemctl daemon-reload"));
        // 凭据不再作为远端进程 argv($1/$2)读取,由 stdin 变量前缀注入
        assert!(!script.contains("PASSWORD=\"$1\""));
        assert!(!script.contains("OBFS_PASSWORD=\"$2\""));
    }

    #[test]
    fn probe_script_takes_fallback_obfs_from_stdin_vars_not_argv() {
        let script = remote_hysteria_probe_script();

        assert!(!script.contains("FALLBACK_OBFS_PASSWORD=\"$1\""));
        // 脚本体仍消费该变量(由 stdin 前缀注入)
        assert!(script.contains("$FALLBACK_OBFS_PASSWORD"));
    }

    #[test]
    fn shell_vars_prefix_quotes_hex_values() {
        let script = with_shell_vars(
            "set -euo pipefail\nbody",
            &[("PASSWORD", "deadbeef"), ("OBFS_PASSWORD", "cafe")],
        );

        assert!(script.starts_with("PASSWORD='deadbeef'\nOBFS_PASSWORD='cafe'\n"));
        assert!(script.ends_with("set -euo pipefail\nbody"));
    }
}
