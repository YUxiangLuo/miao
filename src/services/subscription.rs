use crate::error::{AppError, AppResult};
use crate::models::{AnyTls, Hysteria2, Shadowsocks, Tls};

pub async fn fetch_sub(
    link: &str,
) -> AppResult<(Vec<String>, Vec<serde_json::Value>)> {
    let res = crate::state::CLIENT
        .get(link)
        .timeout(std::time::Duration::from_secs(30))
        .header("User-Agent", "clash-meta")
        .send()
        .await
        .map_err(|e| AppError::context(format!("Failed to fetch subscription from {}", link), e))?;

    let text = res
        .text()
        .await
        .map_err(|e| AppError::context(format!("Failed to read subscription response from {}", link), e))?;

    parse_subscription_content(&text)
        .map_err(|e| AppError::context(format!("Failed to parse subscription content from {}", link), e))
}

fn parse_subscription_content(text: &str) -> AppResult<(Vec<String>, Vec<serde_json::Value>)> {
    let clash_obj: serde_yaml::Value = serde_yaml::from_str(text)
        .map_err(|e| AppError::context("Failed to parse subscription YAML", e))?;

    let proxies = clash_obj
        .get("proxies")
        .and_then(|p| p.as_sequence())
        .unwrap_or(&vec![])
        .clone();

    let nodes: Vec<serde_yaml::Value> = proxies.into_iter().collect();
    let mut node_names = vec![];
    let mut outbounds = vec![];

    for node in nodes {
        let typ = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
        match typ {
            "hysteria2" => {
                let hysteria2 = Hysteria2 {
                    outbound_type: "hysteria2".to_string(),
                    tag: name.to_string(),
                    server: node
                        .get("server")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    server_port: node.get("port").and_then(|p| p.as_u64()).unwrap_or(0)
                        as u16,
                    password: node
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string(),
                    up_mbps: 40,
                    down_mbps: 350,
                    tls: Tls {
                        enabled: true,
                        server_name: node
                            .get("sni")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string()),
                        insecure: node
                            .get("skip-cert-verify")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    },
                };
                node_names.push(name.to_string());
                outbounds.push(
                    serde_json::to_value(hysteria2)
                        .map_err(|e| AppError::context("Failed to serialize parsed hysteria2 node", e))?,
                );
            }
            "anytls" => {
                let anytls = AnyTls {
                    outbound_type: "anytls".to_string(),
                    tag: name.to_string(),
                    server: node
                        .get("server")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    server_port: node.get("port").and_then(|p| p.as_u64()).unwrap_or(0)
                        as u16,
                    password: node
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string(),
                    tls: Tls {
                        enabled: true,
                        server_name: node
                            .get("sni")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string()),
                        insecure: node
                            .get("skip-cert-verify")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    },
                };
                node_names.push(name.to_string());
                outbounds.push(
                    serde_json::to_value(anytls)
                        .map_err(|e| AppError::context("Failed to serialize parsed anytls node", e))?,
                );
            }
            "ss" => {
                let ss = Shadowsocks {
                    outbound_type: "shadowsocks".to_string(),
                    tag: name.to_string(),
                    server: node
                        .get("server")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    server_port: node.get("port").and_then(|p| p.as_u64()).unwrap_or(0)
                        as u16,
                    method: node
                        .get("cipher")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string(),
                    password: node
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string(),
                };
                node_names.push(name.to_string());
                outbounds.push(
                    serde_json::to_value(ss)
                        .map_err(|e| AppError::context("Failed to serialize parsed shadowsocks node", e))?,
                );
            }
            _ => {}
        }
    }
    Ok((node_names, outbounds))
}

#[cfg(test)]
mod tests {
    use super::parse_subscription_content;

    #[test]
    fn parse_subscription_content_extracts_supported_nodes() {
        let yaml = r#"
proxies:
  - name: hy2-node
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass-hy
    sni: hy.example.com
  - name: anytls-node
    type: anytls
    server: any.example.com
    port: 8443
    password: pass-any
    sni: any.example.com
    skip-cert-verify: true
  - name: ss-node
    type: ss
    server: ss.example.com
    port: 8388
    cipher: 2022-blake3-aes-128-gcm
    password: pass-ss
  - name: ignored-node
    type: vmess
    server: vmess.example.com
    port: 443
"#;

        let (names, outbounds) = parse_subscription_content(yaml).unwrap();

        assert_eq!(names, vec!["hy2-node", "anytls-node", "ss-node"]);
        assert_eq!(outbounds.len(), 3);

        assert_eq!(outbounds[0]["type"], "hysteria2");
        assert_eq!(outbounds[0]["tag"], "hy2-node");
        assert_eq!(outbounds[0]["tls"]["server_name"], "hy.example.com");
        assert_eq!(outbounds[1]["type"], "anytls");
        assert_eq!(outbounds[1]["tls"]["insecure"], true);
        assert_eq!(outbounds[2]["type"], "shadowsocks");
        assert_eq!(outbounds[2]["method"], "2022-blake3-aes-128-gcm");
    }

    #[test]
    fn parse_subscription_content_returns_empty_when_proxies_missing() {
        let yaml = "mixed-port: 7890";

        let (names, outbounds) = parse_subscription_content(yaml).unwrap();

        assert!(names.is_empty());
        assert!(outbounds.is_empty());
    }

    #[test]
    fn parse_subscription_content_reports_invalid_yaml() {
        let err = parse_subscription_content("proxies: [").unwrap_err();

        assert!(err.to_string().contains("Failed to parse subscription YAML"));
    }
}
