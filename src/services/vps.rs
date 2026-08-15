use std::io::Read;
use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, Hysteria2, Hysteria2Obfs, Tls};
use crate::services::config::save_config_to;
use crate::services::node_parser::parse_node_json;
use crate::validation::Validator;

const HYSTERIA_PORT: u16 = 543;
const HYSTERIA_OBFS_TYPE: &str = "gecko";
const SSH_CONNECT_TIMEOUT_SECS: &str = "10";
const SSH_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const SSH_PROVISION_TIMEOUT: Duration = Duration::from_secs(300);

/// SSH_ASKPASS 临时文件:密码写 600 权限文件,askpass 脚本(700)cat 它。
/// 密码不出现在进程 argv;文件随作用域删除,不落配置。
/// (OpenSSH >= 8.4 的 SSH_ASKPASS_REQUIRE=force,无需 sshpass 依赖)
struct AskpassFiles {
    script_path: std::path::PathBuf,
    password_path: std::path::PathBuf,
}

impl AskpassFiles {
    fn new(password: &str) -> AppResult<Self> {
        use std::os::unix::fs::PermissionsExt;

        let id = random_password()?;
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("miao-askpass-{id}.sh"));
        let password_path = temp_dir.join(format!("miao-askpass-{id}.pw"));
        let cleanup_script = script_path.clone();
        let cleanup_password = password_path.clone();

        let result = (|| -> AppResult<Self> {
            std::fs::write(&password_path, password)
                .map_err(|e| AppError::context("Failed to write askpass password file", e))?;
            std::fs::set_permissions(&password_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| AppError::context("Failed to secure askpass password file", e))?;
            std::fs::write(
                &script_path,
                format!("#!/bin/sh\ncat \"{}\"\n", password_path.display()),
            )
            .map_err(|e| AppError::context("Failed to write askpass script", e))?;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o700))
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

/// 构建 ssh 命令;密码认证通过 env 注入 askpass,密码不进 argv
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

/// 通过 root 密码 SSH 在 VPS 上部署(或取回已有)Hysteria2 节点并写回配置。
/// 密码只用于本次部署,不会被持久化。
pub async fn deploy_vps_node(
    config: &mut Config,
    config_path: &Path,
    vps_ip: &str,
    root_password: &str,
) -> AppResult<bool> {
    let vps_ip = vps_ip.trim().to_string();

    Validator::server_address(&vps_ip)
        .map_err(|e| AppError::message(format!("Invalid VPS address '{}': {}", vps_ip, e)))?;

    if node_tag_for_vps(config, &vps_ip).is_some() {
        info!(vps_ip = %vps_ip, "Manual node for VPS already exists, skipping provisioning");
        return Ok(false);
    }

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

    config.nodes.push(build_hysteria_node_json(
        &vps_ip,
        &credentials.password,
        &credentials.obfs_password,
    )?);
    save_config_to(config_path, config).await?;
    info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, config_path = ?config_path, "Added provisioned VPS Hysteria2 node to config");

    Ok(true)
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
    let mut child = build_ssh_command(
        vps_ip,
        &askpass,
        &["bash", "-s", "--", fallback_obfs_password],
    )
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| AppError::context("Failed to start ssh for VPS config probe", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(remote_hysteria_probe_script().as_bytes())
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
    let mut child = build_ssh_command(
        vps_ip,
        &askpass,
        &[
            "bash",
            "-s",
            "--",
            &credentials.password,
            &credentials.obfs_password,
        ],
    )
    .stdin(Stdio::piped())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit())
    .spawn()
    .map_err(|e| AppError::context("Failed to start ssh for VPS provisioning", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(remote_hysteria_script().as_bytes())
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
    r#"set -euo pipefail
FALLBACK_OBFS_PASSWORD="$1"
CONFIG="/etc/hysteria/config.yaml"
SERVICE="hysteria-server.service"

if [ "$(id -u)" -ne 0 ]; then
  echo "Miao VPS config probe requires root SSH access" >&2
  exit 20
fi

if [ ! -f "$CONFIG" ]; then
  exit 10
fi

for cmd in awk grep openssl systemctl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 11
  fi
done

# 仅当证书是 miao 部署时生成的(CN=miao-hysteria)才复用配置,否则视为
# 第三方部署,需要清理后重新部署。
if [ ! -f /etc/hysteria/server.crt ]; then
  echo "Existing Hysteria2 service has no /etc/hysteria/server.crt; it is not deployed by Miao and will be cleaned up" >&2
  exit 30
fi

if ! openssl x509 -in /etc/hysteria/server.crt -noout -subject 2>/dev/null | grep -Eq 'CN[[:space:]]*=[[:space:]]*miao-hysteria'; then
  SUBJECT="$(openssl x509 -in /etc/hysteria/server.crt -noout -subject 2>/dev/null || true)"
  echo "Existing Hysteria2 service cert ($SUBJECT) is not signed for miao-hysteria; it is not deployed by Miao and will be cleaned up" >&2
  exit 30
fi

if ! awk '
  /^[[:space:]]*listen:[[:space:]]*:543([[:space:]]|$)/ { found = 1 }
  END { exit found ? 0 : 1 }
' "$CONFIG"; then
  echo "Existing Hysteria2 config does not listen on :543" >&2
  exit 12
fi

PASSWORD="$(awk '
  /^[^[:space:]][^:]*:/ {
    top = $1
    sub(/:$/, "", top)
  }
  top == "auth" && /^[[:space:]]*password:[[:space:]]*/ {
    sub(/^[[:space:]]*password:[[:space:]]*/, "", $0)
    gsub(/^[\"\047]|[\"\047]$/, "", $0)
    print
    found = 1
    exit
  }
  END { if (!found) exit 1 }
' "$CONFIG")"

OBFS_TYPE="$(awk '
  /^[^[:space:]][^:]*:/ {
    top = $1
    sub(/:$/, "", top)
  }
  top == "obfs" && /^[[:space:]]*type:[[:space:]]*/ {
    sub(/^[[:space:]]*type:[[:space:]]*/, "", $0)
    gsub(/^[\"\047]|[\"\047]$/, "", $0)
    print
    found = 1
    exit
  }
  END { if (!found) exit 1 }
' "$CONFIG" || true)"

GECKO_PASSWORD="$(awk '
  /^[^[:space:]][^:]*:/ {
    top = $1
    sub(/:$/, "", top)
    if (top != "obfs") in_gecko = 0
  }
  top == "obfs" && /^[[:space:]]*gecko:[[:space:]]*$/ {
    in_gecko = 1
    next
  }
  top == "obfs" && in_gecko && /^[[:space:]]*password:[[:space:]]*/ {
    sub(/^[[:space:]]*password:[[:space:]]*/, "", $0)
    gsub(/^[\"\047]|[\"\047]$/, "", $0)
    print
    found = 1
    exit
  }
  END { if (!found) exit 1 }
' "$CONFIG" || true)"

if [ -z "$PASSWORD" ]; then
  echo "Existing Hysteria2 config has no password" >&2
  exit 13
fi

if [ "$OBFS_TYPE" != "gecko" ] || [ -z "$GECKO_PASSWORD" ]; then
  if [ ! -f /etc/hysteria/server.crt ] || [ ! -f /etc/hysteria/server.key ]; then
    echo "Existing Hysteria2 config cannot be upgraded to Gecko obfs without default cert files" >&2
    exit 14
  fi

  GECKO_PASSWORD="$FALLBACK_OBFS_PASSWORD"
  cat > "$CONFIG" <<EOF
listen: :543
tls:
  cert: /etc/hysteria/server.crt
  key: /etc/hysteria/server.key
auth:
  type: password
  password: ${PASSWORD}
obfs:
  type: gecko
  gecko:
    password: ${GECKO_PASSWORD}
masquerade:
  type: proxy
  proxy:
    url: https://www.bing.com/
    rewriteHost: true
EOF
  chmod 600 "$CONFIG"
fi

systemctl enable "$SERVICE" >/dev/null 2>&1 || true
systemctl restart "$SERVICE"
systemctl is-active --quiet "$SERVICE"
printf '%s\n' "$PASSWORD"
printf '%s\n' "$GECKO_PASSWORD"
"#
}

fn remote_hysteria_script() -> &'static str {
    r#"set -euo pipefail
PASSWORD="$1"
OBFS_PASSWORD="$2"
SERVICE="hysteria-server.service"

if [ "$(id -u)" -ne 0 ]; then
  echo "Miao VPS provisioning requires root SSH access" >&2
  exit 1
fi

for cmd in bash curl systemctl openssl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 1
  fi
done

# 重新部署前先清理远程可能存在的非 Miao 部署的 hysteria 残留,
# 保证从干净状态开始(探针复用失败时也会经过这里)。
systemctl stop "$SERVICE" >/dev/null 2>&1 || true
systemctl disable "$SERVICE" >/dev/null 2>&1 || true
pkill -x hysteria >/dev/null 2>&1 || true
rm -rf /etc/hysteria
rm -f /usr/local/bin/hysteria

HYSTERIA_USER=root bash <(curl -fsSL https://get.hy2.sh/)

install -d -m 700 /etc/hysteria
openssl req -x509 -nodes -newkey rsa:2048 -sha256 -days 3650 \
  -keyout /etc/hysteria/server.key \
  -out /etc/hysteria/server.crt \
  -subj "/CN=miao-hysteria" >/dev/null 2>&1
chmod 600 /etc/hysteria/server.key
chmod 644 /etc/hysteria/server.crt

cat > /etc/hysteria/config.yaml <<EOF
listen: :543
tls:
  cert: /etc/hysteria/server.crt
  key: /etc/hysteria/server.key
auth:
  type: password
  password: ${PASSWORD}
obfs:
  type: gecko
  gecko:
    password: ${OBFS_PASSWORD}
masquerade:
  type: proxy
  proxy:
    url: https://www.bing.com/
    rewriteHost: true
EOF
chmod 600 /etc/hysteria/config.yaml

systemctl enable "$SERVICE"
systemctl restart "$SERVICE"
systemctl is-active --quiet "$SERVICE"
"#
}

#[cfg(test)]
mod tests {
    use super::{
        build_hysteria_node_json, build_ssh_command, node_tag_for_vps, parse_probe_credentials,
        remote_hysteria_probe_script, remote_hysteria_script, vps_node_tag, AskpassFiles,
        HYSTERIA_PORT,
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
            adblock: false,
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
}
