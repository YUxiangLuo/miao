use std::io::Read;
use std::process::Stdio;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, Hysteria2, Hysteria2Obfs, Tls};
use crate::services::config::save_config;
use crate::services::node_parser::parse_node_json;
use crate::validation::Validator;

const HYSTERIA_PORT: u16 = 543;
const HYSTERIA_OBFS_TYPE: &str = "gecko";
const SSH_CONNECT_TIMEOUT_SECS: &str = "10";
const SSH_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const SSH_PROVISION_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug)]
struct HysteriaCredentials {
    password: String,
    obfs_password: String,
}

pub fn has_manual_node_for_vps(config: &Config) -> bool {
    let Some(vps_ip) = config
        .vps_ip
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return false;
    };

    config.nodes.iter().any(|node| {
        parse_node_json(node)
            .map(|(info, _)| info.server == vps_ip)
            .unwrap_or(false)
    })
}

pub async fn ensure_vps_hysteria_node(config: &mut Config) -> AppResult<bool> {
    let Some(vps_ip) = config
        .vps_ip
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return Ok(false);
    };
    let vps_ip = vps_ip.to_string();

    Validator::server_address(&vps_ip)
        .map_err(|e| AppError::message(format!("Invalid vps_ip '{}': {}", vps_ip, e)))?;

    if config.vps_ip.as_deref() != Some(vps_ip.as_str()) {
        config.vps_ip = Some(vps_ip.clone());
    }

    if has_manual_node_for_vps(config) {
        info!(vps_ip = %vps_ip, "Manual node for vps_ip already exists, skipping VPS provisioning");
        return Ok(false);
    }

    let fallback_obfs_password = random_password()?;
    let credentials = match probe_remote_hysteria_credentials(&vps_ip, &fallback_obfs_password)
        .await
    {
        Ok(Some(credentials)) => {
            info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, obfs = HYSTERIA_OBFS_TYPE, "Recovered existing VPS Hysteria2 node from remote config");
            credentials
        }
        Ok(None) => {
            let credentials = HysteriaCredentials {
                password: random_password()?,
                obfs_password: fallback_obfs_password,
            };
            info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, obfs = HYSTERIA_OBFS_TYPE, "Provisioning Hysteria2 server over SSH");
            provision_remote_hysteria(&vps_ip, &credentials).await?;
            credentials
        }
        Err(e) => {
            warn!(vps_ip = %vps_ip, error = %e, "Failed to probe existing Hysteria2 config, falling back to provisioning");
            let credentials = HysteriaCredentials {
                password: random_password()?,
                obfs_password: fallback_obfs_password,
            };
            info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, obfs = HYSTERIA_OBFS_TYPE, "Provisioning Hysteria2 server over SSH");
            provision_remote_hysteria(&vps_ip, &credentials).await?;
            credentials
        }
    };

    config.nodes.push(build_hysteria_node_json(
        &vps_ip,
        &credentials.password,
        &credentials.obfs_password,
    )?);
    save_config(config).await?;
    info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, "Added provisioned VPS Hysteria2 node to config.yaml");

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
) -> AppResult<Option<HysteriaCredentials>> {
    let target = format!("root@{}", vps_ip);
    let mut child = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            &format!("ConnectTimeout={SSH_CONNECT_TIMEOUT_SECS}"),
            &target,
            "bash",
            "-s",
            "--",
            fallback_obfs_password,
        ])
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
        return parse_probe_credentials(&stdout_buf).map(Some);
    }

    if status.code() == Some(10) {
        info!(vps_ip = %vps_ip, "No reusable remote Hysteria2 config found");
        return Ok(None);
    }

    let stderr_text = String::from_utf8_lossy(&stderr_buf);
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
) -> AppResult<()> {
    let target = format!("root@{}", vps_ip);
    let mut child = tokio::process::Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            &format!("ConnectTimeout={SSH_CONNECT_TIMEOUT_SECS}"),
            &target,
            "bash",
            "-s",
            "--",
            &credentials.password,
            &credentials.obfs_password,
        ])
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

for cmd in awk systemctl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required command: $cmd" >&2
    exit 11
  fi
done

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

systemctl enable hysteria-server.service
systemctl restart hysteria-server.service
systemctl is-active --quiet hysteria-server.service
"#
}

#[cfg(test)]
mod tests {
    use super::{
        build_hysteria_node_json, has_manual_node_for_vps, parse_probe_credentials,
        remote_hysteria_probe_script, vps_node_tag, HYSTERIA_PORT,
    };
    use crate::models::Config;

    #[test]
    fn detects_existing_manual_node_for_vps_ip() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"manual","server":"203.0.113.10","server_port":543,"password":"secret","tls":{"enabled":true,"insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
            vps_ip: Some("203.0.113.10".to_string()),
        };

        assert!(has_manual_node_for_vps(&config));
    }

    #[test]
    fn ignores_invalid_manual_nodes_when_checking_vps_ip() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec!["not-json".to_string()],
            custom_rules: vec![],
            route_mode: Default::default(),
            vps_ip: Some("203.0.113.10".to_string()),
        };

        assert!(!has_manual_node_for_vps(&config));
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
}
