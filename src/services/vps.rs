use std::io::Read;
use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::time::{timeout, Duration};
use tracing::info;

use crate::error::{AppError, AppResult};
use crate::models::{Config, Hysteria2, Tls};
use crate::services::config::save_config;
use crate::services::node_parser::parse_node_json;
use crate::validation::Validator;

const HYSTERIA_PORT: u16 = 543;
const SSH_CONNECT_TIMEOUT_SECS: &str = "10";
const SSH_PROVISION_TIMEOUT: Duration = Duration::from_secs(300);

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

    let password = random_password()?;
    info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, "Provisioning Hysteria2 server over SSH");
    provision_remote_hysteria(&vps_ip, &password).await?;

    config
        .nodes
        .push(build_hysteria_node_json(&vps_ip, &password)?);
    save_config(config).await?;
    info!(vps_ip = %vps_ip, port = HYSTERIA_PORT, "Added provisioned VPS Hysteria2 node to config.yaml");

    Ok(true)
}

fn build_hysteria_node_json(server: &str, password: &str) -> AppResult<String> {
    let node = Hysteria2 {
        outbound_type: "hysteria2".to_string(),
        tag: vps_node_tag(server),
        server: server.to_string(),
        server_port: HYSTERIA_PORT,
        password: password.to_string(),
        up_mbps: None,
        down_mbps: None,
        obfs: None,
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

async fn provision_remote_hysteria(vps_ip: &str, password: &str) -> AppResult<()> {
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
            password,
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

fn remote_hysteria_script() -> &'static str {
    r#"set -euo pipefail
PASSWORD="$1"

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
    use super::{build_hysteria_node_json, has_manual_node_for_vps, vps_node_tag, HYSTERIA_PORT};
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
            vps_ip: Some("203.0.113.10".to_string()),
        };

        assert!(!has_manual_node_for_vps(&config));
    }

    #[test]
    fn builds_hysteria2_node_for_self_signed_vps() {
        let node = build_hysteria_node_json("203.0.113.10", "password123").unwrap();
        let value: serde_json::Value = serde_json::from_str(&node).unwrap();

        assert_eq!(value["type"], "hysteria2");
        assert_eq!(value["tag"], "vps-203-0-113-10");
        assert_eq!(value["server"], "203.0.113.10");
        assert_eq!(value["server_port"], HYSTERIA_PORT);
        assert_eq!(value["password"], "password123");
        assert_eq!(value["tls"]["enabled"], true);
        assert_eq!(value["tls"]["insecure"], true);
    }

    #[test]
    fn vps_node_tag_is_stable_and_limited() {
        assert_eq!(vps_node_tag("Example.COM"), "vps-example-com");
        assert!(vps_node_tag(&"a".repeat(100)).len() <= 64);
    }
}
