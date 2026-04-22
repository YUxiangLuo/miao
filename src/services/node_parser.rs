use serde_yaml::Value;

use crate::error::{AppError, AppResult};

/// 节点解析结果，包含有效节点和错误记录
#[derive(Debug)]
pub struct ParseResult {
    pub nodes: Vec<(String, serde_json::Value)>, // (name, outbound_json)
    pub errors: Vec<String>,                     // 记录解析失败的节点及原因
    pub total_count: usize,                      // YAML 中 proxies 列表的原始总数
}

/// 从 Clash 配置中解析节点，跳过无效节点并记录错误
pub fn parse_clash_proxies(clash_yaml: &str) -> AppResult<ParseResult> {
    let clash_obj: Value = serde_yaml::from_str(clash_yaml)
        .map_err(|e| AppError::context("Failed to parse subscription YAML", e))?;

    let proxies = clash_obj
        .get("proxies")
        .and_then(|p| p.as_sequence())
        .cloned()
        .unwrap_or_default();

    let mut result = ParseResult {
        nodes: vec![],
        errors: vec![],
        total_count: proxies.len(),
    };

    for (idx, node) in proxies.iter().enumerate() {
        let node_type = node
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");

        // Skip unsupported node types silently
        if !is_supported_node_type(node_type) {
            continue;
        }

        match parse_single_node(node) {
            Some((name, outbound)) => result.nodes.push((name, outbound)),
            None => {
                let name = node
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("<index {}>", idx));
                result.errors.push(format!(
                    "Node '{}' (type: {}): missing required fields (type/server/port/password)",
                    name, node_type
                ));
            }
        }
    }

    Ok(result)
}

fn is_supported_node_type(node_type: &str) -> bool {
    matches!(node_type, "hysteria2" | "anytls" | "ss" | "trojan" | "vmess" | "vless" | "tuic")
}

fn parse_single_node(node: &Value) -> Option<(String, serde_json::Value)> {
    let typ = node.get("type")?.as_str()?;
    let name = node.get("name")?.as_str()?;

    // 验证必需字段
    let server = node.get("server")?.as_str()?;
    let port = node.get("port")?.as_u64()?;
    if port == 0 || port > 65535 {
        return None;
    }

    let outbound = match typ {
        "hysteria2" => {
            let password = node.get("password")?.as_str()?;
            let sni = node.get("sni").and_then(|s| s.as_str());
            let insecure = node
                .get("skip-cert-verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut obj = serde_json::json!({
                "type": "hysteria2",
                "tag": name,
                "server": server,
                "server_port": port,
                "password": password,
                "tls": {
                    "enabled": true,
                    "insecure": insecure
                }
            });

            if let Some(sni_val) = sni {
                obj["tls"]["server_name"] = serde_json::Value::String(sni_val.to_string());
            }

            obj
        }
        "anytls" => {
            let password = node.get("password")?.as_str()?;
            let sni = node.get("sni").and_then(|s| s.as_str());
            let insecure = node
                .get("skip-cert-verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut obj = serde_json::json!({
                "type": "anytls",
                "tag": name,
                "server": server,
                "server_port": port,
                "password": password,
                "tls": {
                    "enabled": true,
                    "insecure": insecure
                }
            });

            if let Some(sni_val) = sni {
                obj["tls"]["server_name"] = serde_json::Value::String(sni_val.to_string());
            }

            obj
        }
        "ss" => {
            let password = node.get("password")?.as_str()?;
            let method = node.get("cipher")?.as_str()?;
            serde_json::json!({
                "type": "shadowsocks",
                "tag": name,
                "server": server,
                "server_port": port,
                "method": method,
                "password": password
            })
        }
        "trojan" => {
            let password = node.get("password")?.as_str()?;
            let sni = node.get("sni").and_then(|s| s.as_str());
            let insecure = node
                .get("skip-cert-verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut obj = serde_json::json!({
                "type": "trojan",
                "tag": name,
                "server": server,
                "server_port": port,
                "password": password,
                "tls": {
                    "enabled": true,
                    "insecure": insecure
                }
            });

            if let Some(sni_val) = sni {
                obj["tls"]["server_name"] = serde_json::Value::String(sni_val.to_string());
            }

            obj
        }
        "vmess" => {
            let uuid = node
                .get("uuid")
                .or_else(|| node.get("password"))
                ?.as_str()?;
            let security = node.get("cipher").and_then(|s| s.as_str()).unwrap_or("auto");
            let alter_id = node.get("alterId").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let tls_enabled = node
                .get("tls")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let sni = node
                .get("servername")
                .or_else(|| node.get("sni"))
                .and_then(|s| s.as_str());
            let insecure = node
                .get("skip-cert-verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut obj = serde_json::json!({
                "type": "vmess",
                "tag": name,
                "server": server,
                "server_port": port,
                "uuid": uuid,
                "security": security,
                "alter_id": alter_id
            });

            if tls_enabled {
                let mut tls_obj = serde_json::json!({
                    "enabled": true,
                    "insecure": insecure
                });
                if let Some(sni_val) = sni {
                    tls_obj["server_name"] = serde_json::Value::String(sni_val.to_string());
                }
                obj["tls"] = tls_obj;
            }

            obj
        }
        "vless" => {
            let uuid = node.get("uuid")?.as_str()?;
            let flow = node.get("flow").and_then(|s| s.as_str());
            let tls_enabled = node
                .get("tls")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let sni = node
                .get("servername")
                .or_else(|| node.get("sni"))
                .and_then(|s| s.as_str());
            let insecure = node
                .get("skip-cert-verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut obj = serde_json::json!({
                "type": "vless",
                "tag": name,
                "server": server,
                "server_port": port,
                "uuid": uuid
            });

            if let Some(flow_val) = flow {
                obj["flow"] = serde_json::Value::String(flow_val.to_string());
            }

            if tls_enabled {
                let mut tls_obj = serde_json::json!({
                    "enabled": true,
                    "insecure": insecure
                });
                if let Some(sni_val) = sni {
                    tls_obj["server_name"] = serde_json::Value::String(sni_val.to_string());
                }
                obj["tls"] = tls_obj;
            }

            obj
        }
        "tuic" => {
            // TUIC v5: sing-box uses `uuid` field; Clash Meta may use `token` or `uuid`
            let token = node
                .get("token")
                .or_else(|| node.get("uuid"))
                ?.as_str()?;
            let congestion_control = node
                .get("congestion-controller")
                .and_then(|s| s.as_str())
                .unwrap_or("bbr");
            let sni = node
                .get("sni")
                .and_then(|s| s.as_str());
            let insecure = node
                .get("skip-cert-verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut obj = serde_json::json!({
                "type": "tuic",
                "tag": name,
                "server": server,
                "server_port": port,
                "uuid": token,
                "congestion_control": congestion_control
            });

            let mut tls_obj = serde_json::json!({
                "enabled": true,
                "insecure": insecure
            });
            if let Some(sni_val) = sni {
                tls_obj["server_name"] = serde_json::Value::String(sni_val.to_string());
            }
            obj["tls"] = tls_obj;

            obj
        }
        _ => return None, // 不支持的类型
    };

    Some((name.to_string(), outbound))
}

/// 解析单个节点 JSON 字符串，返回验证后的 Value 和显示信息
pub fn parse_node_json(node_str: &str) -> Result<(NodeDisplayInfo, serde_json::Value), String> {
    let v: serde_json::Value =
        serde_json::from_str(node_str).map_err(|e| format!("Invalid JSON: {}", e))?;

    let tag = v
        .get("tag")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("Missing or empty tag")?
        .to_string();

    let server = v
        .get("server")
        .and_then(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("Missing or empty server")?
        .to_string();

    let server_port = v
        .get("server_port")
        .and_then(|p| p.as_u64())
        .and_then(|p| {
            if p > 0 && p <= 65535 {
                Some(p as u16)
            } else {
                None
            }
        })
        .ok_or("Invalid or missing port")?;

    let node_type = v
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown")
        .to_string();

    let sni = v
        .get("tls")
        .and_then(|t| t.get("server_name"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let info = NodeDisplayInfo {
        tag,
        server,
        server_port,
        node_type,
        sni,
    };

    Ok((info, v))
}

/// 节点显示信息结构
#[derive(Debug, Clone)]
pub struct NodeDisplayInfo {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub node_type: String,
    pub sni: Option<String>,
}

impl std::fmt::Display for NodeDisplayInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({}:{}) [{}]",
            self.tag, self.server, self.server_port, self.node_type
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clash_proxies_extracts_valid_nodes() {
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

        let result = parse_clash_proxies(yaml).unwrap();

        // 3 valid nodes + 1 invalid vmess (missing uuid) skipped
        assert_eq!(result.nodes.len(), 3);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("ignored-node"));
        assert_eq!(result.nodes[0].0, "hy2-node");
        assert_eq!(result.nodes[1].0, "anytls-node");
        assert_eq!(result.nodes[2].0, "ss-node");
    }

    #[test]
    fn parse_clash_proxies_skips_invalid_nodes() {
        let yaml = r#"
proxies:
  - name: valid-node
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass-hy
  - name: invalid-missing-server
    type: hysteria2
    port: 443
    password: pass-hy
  - name: invalid-zero-port
    type: hysteria2
    server: hy.example.com
    port: 0
    password: pass-hy
  - name: invalid-missing-password
    type: hysteria2
    server: hy.example.com
    port: 443
  - name: unsupported-type
    type: socks5
    server: socks.example.com
    port: 1080
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].0, "valid-node");
        // 3 errors: missing-server, zero-port, missing-password
        // unsupported-type (socks5) is silently skipped, not reported as error
        assert_eq!(result.errors.len(), 3);

        // Verify error messages contain node names
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("invalid-missing-server")));
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("invalid-zero-port")));
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("invalid-missing-password")));
    }

    #[test]
    fn parse_clash_proxies_returns_empty_for_missing_proxies() {
        let yaml = "mixed-port: 7890";

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn parse_clash_proxies_reports_invalid_yaml() {
        let err = parse_clash_proxies("proxies: [").unwrap_err();

        assert!(err
            .to_string()
            .contains("Failed to parse subscription YAML"));
    }

    #[test]
    fn parse_node_json_extracts_valid_node() {
        let json = r#"{"type":"hysteria2","tag":"test-node","server":"example.com","server_port":443,"password":"secret","tls":{"enabled":true,"server_name":"sni.example.com"}}"#;

        let (info, value) = parse_node_json(json).unwrap();

        assert_eq!(info.tag, "test-node");
        assert_eq!(info.server, "example.com");
        assert_eq!(info.server_port, 443);
        assert_eq!(info.node_type, "hysteria2");
        assert_eq!(info.sni, Some("sni.example.com".to_string()));
        // 验证返回的 Value 是正确的
        assert_eq!(value["tag"], "test-node");
        assert_eq!(value["server"], "example.com");
    }

    #[test]
    fn parse_node_json_rejects_empty_tag() {
        let json = r#"{"type":"hysteria2","tag":"","server":"example.com","server_port":443,"password":"secret"}"#;

        let err = parse_node_json(json).unwrap_err();
        assert!(err.contains("tag"));
    }

    #[test]
    fn parse_node_json_rejects_zero_port() {
        let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":0,"password":"secret"}"#;

        let err = parse_node_json(json).unwrap_err();
        assert!(err.contains("port"));
    }

    #[test]
    fn parse_node_json_rejects_missing_server() {
        let json = r#"{"type":"hysteria2","tag":"test","server_port":443,"password":"secret"}"#;

        let err = parse_node_json(json).unwrap_err();
        assert!(err.contains("server"));
    }

    #[test]
    fn parse_node_json_handles_optional_sni() {
        let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.sni, None);
    }

    #[test]
    fn parse_node_json_handles_missing_tls() {
        let json = r#"{"type":"shadowsocks","tag":"test","server":"example.com","server_port":8388,"password":"secret","method":"aes-128-gcm"}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.sni, None);
    }

    #[test]
    fn parse_node_json_rejects_port_too_large() {
        let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":65536,"password":"secret"}"#;

        let err = parse_node_json(json).unwrap_err();
        assert!(err.contains("port"));
    }

    #[test]
    fn parse_node_json_rejects_max_valid_port() {
        // 65535 should be accepted
        let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":65535,"password":"secret"}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.server_port, 65535);
    }

    #[test]
    fn parse_node_json_accepts_ipv4_server() {
        let json = r#"{"type":"hysteria2","tag":"test","server":"192.168.1.1","server_port":443,"password":"secret"}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.server, "192.168.1.1");
    }

    #[test]
    fn parse_node_json_accepts_ipv6_server() {
        let json = r#"{"type":"hysteria2","tag":"test","server":"::1","server_port":443,"password":"secret"}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.server, "::1");
    }

    #[test]
    fn parse_clash_proxies_handles_all_supported_types() {
        let yaml = r#"
proxies:
  - name: hy2-1
    type: hysteria2
    server: hy1.example.com
    port: 443
    password: pass1
  - name: hy2-2
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: pass2
    sni: hy2.example.com
  - name: anytls-1
    type: anytls
    server: any1.example.com
    port: 8443
    password: pass3
  - name: ss-1
    type: ss
    server: ss1.example.com
    port: 8388
    cipher: aes-256-gcm
    password: pass4
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 4);
        assert!(result.errors.is_empty());

        let types: Vec<String> = result
            .nodes
            .iter()
            .map(|(_, o)| o.get("type").unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            types,
            vec!["hysteria2", "hysteria2", "anytls", "shadowsocks"]
        );
    }

    #[test]
    fn parse_clash_proxies_handles_empty_proxies_list() {
        let yaml = r#"
proxies: []
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn parse_clash_proxies_preserves_skip_cert_verify() {
        let yaml = r#"
proxies:
  - name: test-skip-verify
    type: hysteria2
    server: test.example.com
    port: 443
    password: pass
    skip-cert-verify: true
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["tls"]["insecure"], true);
    }

    #[test]
    fn parse_clash_proxies_defaults_skip_cert_verify_to_false() {
        let yaml = r#"
proxies:
  - name: test-default-verify
    type: hysteria2
    server: test.example.com
    port: 443
    password: pass
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["tls"]["insecure"], false);
    }

    #[test]
    fn parse_clash_proxies_handles_mixed_valid_and_unsupported() {
        let yaml = r#"
proxies:
  - name: valid-hy2
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
  - name: valid-vmess
    type: vmess
    server: vm.example.com
    port: 443
    uuid: bf000d23-0752-40b4-affe-68f7707a9661
  - name: valid-trojan
    type: trojan
    server: tr.example.com
    port: 443
    password: pass
  - name: valid-ss
    type: ss
    server: ss.example.com
    port: 8388
    cipher: aes-128-gcm
    password: pass
  - name: unsupported-socks5
    type: socks5
    server: socks.example.com
    port: 1080
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        // 4 valid nodes + 1 unsupported type (socks5) silently skipped
        assert_eq!(result.nodes.len(), 4);
        assert!(result.errors.is_empty());

        let names: Vec<String> = result.nodes.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(names, vec!["valid-hy2", "valid-vmess", "valid-trojan", "valid-ss"]);
    }

    #[test]
    fn parse_clash_proxies_hysteria2_without_bandwidth_defaults() {
        // 测试：从 Clash 配置解析 Hysteria2 时不添加硬编码带宽
        let yaml = r#"
proxies:
  - name: hy2-without-bandwidth
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
    sni: hy.example.com
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["type"], "hysteria2");
        assert_eq!(outbound["tag"], "hy2-without-bandwidth");
        // 关键测试：不应包含硬编码的 up_mbps/down_mbps
        assert!(outbound.get("up_mbps").is_none() || outbound["up_mbps"].is_null());
        assert!(outbound.get("down_mbps").is_none() || outbound["down_mbps"].is_null());
    }

    #[test]
    fn parse_node_json_rejects_empty_server() {
        let json = r#"{"type":"hysteria2","tag":"test","server":"","server_port":443,"password":"secret"}"#;

        let err = parse_node_json(json).unwrap_err();
        assert!(err.contains("server"));
    }

    #[test]
    fn parse_node_json_rejects_whitespace_only_server() {
        let json = r#"{"type":"hysteria2","tag":"test","server":"   ","server_port":443,"password":"secret"}"#;

        let err = parse_node_json(json).unwrap_err();
        assert!(err.contains("server"));
    }

    #[test]
    fn parse_node_json_accepts_whitespace_in_tag() {
        let json = r#"{"type":"hysteria2","tag":"My Node 1","server":"example.com","server_port":443,"password":"secret"}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.tag, "My Node 1");
    }

    #[test]
    fn parse_clash_proxies_reports_multiple_missing_fields() {
        let yaml = r#"
proxies:
  - name: missing-everything
    type: hysteria2
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("missing-everything"));
    }

    #[test]
    fn parse_clash_proxies_handles_ss_without_cipher() {
        let yaml = r#"
proxies:
  - name: ss-no-cipher
    type: ss
    server: ss.example.com
    port: 8388
    password: pass
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        // SS without cipher should be rejected
        assert!(result.nodes.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("ss-no-cipher"));
    }

    #[test]
    fn parse_node_json_display_format() {
        let info = NodeDisplayInfo {
            tag: "Test Node".to_string(),
            server: "192.168.1.1".to_string(),
            server_port: 8388,
            node_type: "shadowsocks".to_string(),
            sni: None,
        };

        let display = format!("{}", info);
        assert_eq!(display, "Test Node (192.168.1.1:8388) [shadowsocks]");
    }

    #[test]
    fn parse_clash_proxies_extracts_trojan_node() {
        let yaml = r#"
proxies:
  - name: trojan-node
    type: trojan
    server: tr.example.com
    port: 443
    password: trojan-pass
    sni: tr.example.com
    skip-cert-verify: true
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.nodes[0].0, "trojan-node");

        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["type"], "trojan");
        assert_eq!(outbound["tag"], "trojan-node");
        assert_eq!(outbound["server"], "tr.example.com");
        assert_eq!(outbound["server_port"], 443);
        assert_eq!(outbound["password"], "trojan-pass");
        assert_eq!(outbound["tls"]["enabled"], true);
        assert_eq!(outbound["tls"]["server_name"], "tr.example.com");
        assert_eq!(outbound["tls"]["insecure"], true);
    }

    #[test]
    fn parse_clash_proxies_extracts_vmess_node() {
        let yaml = r#"
proxies:
  - name: vmess-node
    type: vmess
    server: vm.example.com
    port: 443
    uuid: bf000d23-0752-40b4-affe-68f7707a9661
    alterId: 0
    cipher: auto
    tls: true
    servername: vm.example.com
    skip-cert-verify: false
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.nodes[0].0, "vmess-node");

        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["type"], "vmess");
        assert_eq!(outbound["tag"], "vmess-node");
        assert_eq!(outbound["server"], "vm.example.com");
        assert_eq!(outbound["server_port"], 443);
        assert_eq!(outbound["uuid"], "bf000d23-0752-40b4-affe-68f7707a9661");
        assert_eq!(outbound["security"], "auto");
        assert_eq!(outbound["alter_id"], 0);
        assert_eq!(outbound["tls"]["enabled"], true);
        assert_eq!(outbound["tls"]["server_name"], "vm.example.com");
        assert_eq!(outbound["tls"]["insecure"], false);
    }

    #[test]
    fn parse_clash_proxies_extracts_vmess_without_tls() {
        let yaml = r#"
proxies:
  - name: vmess-plain
    type: vmess
    server: vm.example.com
    port: 80
    uuid: bf000d23-0752-40b4-affe-68f7707a9661
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["type"], "vmess");
        assert!(outbound.get("tls").is_none());
    }

    #[test]
    fn parse_clash_proxies_vmess_fallback_to_password_as_uuid() {
        // 某些 Clash 配置可能将 uuid 放在 password 字段
        let yaml = r#"
proxies:
  - name: vmess-pwd
    type: vmess
    server: vm.example.com
    port: 443
    password: bf000d23-0752-40b4-affe-68f7707a9661
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["uuid"], "bf000d23-0752-40b4-affe-68f7707a9661");
    }

    #[test]
    fn parse_clash_proxies_skips_trojan_missing_password() {
        let yaml = r#"
proxies:
  - name: trojan-no-pwd
    type: trojan
    server: tr.example.com
    port: 443
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("trojan-no-pwd"));
    }

    #[test]
    fn parse_clash_proxies_skips_vmess_missing_uuid() {
        let yaml = r#"
proxies:
  - name: vmess-no-uuid
    type: vmess
    server: vm.example.com
    port: 443
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("vmess-no-uuid"));
    }

    #[test]
    fn parse_clash_proxies_extracts_vless_node() {
        let yaml = r#"
proxies:
  - name: vless-node
    type: vless
    server: vl.example.com
    port: 443
    uuid: bf000d23-0752-40b4-affe-68f7707a9661
    flow: xtls-rprx-vision
    tls: true
    servername: vl.example.com
    skip-cert-verify: false
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.nodes[0].0, "vless-node");

        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["type"], "vless");
        assert_eq!(outbound["tag"], "vless-node");
        assert_eq!(outbound["server"], "vl.example.com");
        assert_eq!(outbound["server_port"], 443);
        assert_eq!(outbound["uuid"], "bf000d23-0752-40b4-affe-68f7707a9661");
        assert_eq!(outbound["flow"], "xtls-rprx-vision");
        assert_eq!(outbound["tls"]["enabled"], true);
        assert_eq!(outbound["tls"]["server_name"], "vl.example.com");
        assert_eq!(outbound["tls"]["insecure"], false);
    }

    #[test]
    fn parse_clash_proxies_extracts_vless_without_tls() {
        let yaml = r#"
proxies:
  - name: vless-plain
    type: vless
    server: vl.example.com
    port: 80
    uuid: bf000d23-0752-40b4-affe-68f7707a9661
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["type"], "vless");
        assert!(outbound.get("tls").is_none());
    }

    #[test]
    fn parse_clash_proxies_extracts_tuic_node() {
        let yaml = r#"
proxies:
  - name: tuic-node
    type: tuic
    server: tu.example.com
    port: 443
    token: my-secret-token
    uuid: bf000d23-0752-40b4-affe-68f7707a9661
    congestion-controller: bbr
    sni: tu.example.com
    skip-cert-verify: true
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert!(result.errors.is_empty());
        assert_eq!(result.nodes[0].0, "tuic-node");

        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["type"], "tuic");
        assert_eq!(outbound["tag"], "tuic-node");
        assert_eq!(outbound["server"], "tu.example.com");
        assert_eq!(outbound["server_port"], 443);
        assert_eq!(outbound["uuid"], "my-secret-token");
        assert_eq!(outbound["congestion_control"], "bbr");
        assert_eq!(outbound["tls"]["enabled"], true);
        assert_eq!(outbound["tls"]["server_name"], "tu.example.com");
        assert_eq!(outbound["tls"]["insecure"], true);
    }

    #[test]
    fn parse_clash_proxies_extracts_tuic_with_uuid_fallback() {
        let yaml = r#"
proxies:
  - name: tuic-uuid
    type: tuic
    server: tu.example.com
    port: 443
    uuid: bf000d23-0752-40b4-affe-68f7707a9661
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        let outbound = &result.nodes[0].1;
        assert_eq!(outbound["uuid"], "bf000d23-0752-40b4-affe-68f7707a9661");
        assert_eq!(outbound["congestion_control"], "bbr");
    }

    #[test]
    fn parse_clash_proxies_skips_vless_missing_uuid() {
        let yaml = r#"
proxies:
  - name: vless-no-uuid
    type: vless
    server: vl.example.com
    port: 443
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("vless-no-uuid"));
    }

    #[test]
    fn parse_clash_proxies_skips_tuic_missing_token() {
        let yaml = r#"
proxies:
  - name: tuic-no-token
    type: tuic
    server: tu.example.com
    port: 443
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("tuic-no-token"));
    }

    #[test]
    fn parse_node_json_extracts_trojan_info() {
        let json = r#"{"type":"trojan","tag":"tr-test","server":"tr.example.com","server_port":443,"password":"secret","tls":{"enabled":true,"server_name":"sni.example.com"}}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.tag, "tr-test");
        assert_eq!(info.server, "tr.example.com");
        assert_eq!(info.server_port, 443);
        assert_eq!(info.node_type, "trojan");
        assert_eq!(info.sni, Some("sni.example.com".to_string()));
    }

    #[test]
    fn parse_node_json_extracts_vmess_info() {
        let json = r#"{"type":"vmess","tag":"vm-test","server":"vm.example.com","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","security":"auto","tls":{"enabled":true,"server_name":"sni.example.com"}}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.tag, "vm-test");
        assert_eq!(info.server, "vm.example.com");
        assert_eq!(info.node_type, "vmess");
        assert_eq!(info.sni, Some("sni.example.com".to_string()));
    }

    #[test]
    fn parse_node_json_extracts_vless_info() {
        let json = r#"{"type":"vless","tag":"vl-test","server":"vl.example.com","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","flow":"xtls-rprx-vision","tls":{"enabled":true}}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.tag, "vl-test");
        assert_eq!(info.server, "vl.example.com");
        assert_eq!(info.node_type, "vless");
    }

    #[test]
    fn parse_node_json_extracts_tuic_info() {
        let json = r#"{"type":"tuic","tag":"tu-test","server":"tu.example.com","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","congestion_control":"bbr","tls":{"enabled":true}}"#;

        let (info, _) = parse_node_json(json).unwrap();
        assert_eq!(info.tag, "tu-test");
        assert_eq!(info.server, "tu.example.com");
        assert_eq!(info.node_type, "tuic");
    }
}
