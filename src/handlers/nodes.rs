use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::Arc;
use tracing::warn;

use crate::models::{
    AnyTls, ApiResponse, DeleteNodeRequest, Hysteria2, NodeInfo, NodeRequest, Shadowsocks, Tls,
    Trojan, Tuic, VMess, Vless,
};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::apply_config_change;
use crate::services::node_parser::parse_node_json;
use crate::state::AppState;
use crate::validation::{Validator, VALID_VMESS_SECURITY};

pub async fn get_nodes(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<NodeInfo>>> {
    let config = state.config.read().await;

    let mut nodes = Vec::new();
    let mut parse_errors = Vec::new();

    for (idx, node_str) in config.nodes.iter().enumerate() {
        match parse_node_json(node_str) {
            Ok((display_info, _)) => {
                nodes.push(NodeInfo {
                    tag: display_info.tag,
                    server: display_info.server,
                    server_port: display_info.server_port,
                    node_type: display_info.node_type,
                    sni: display_info.sni,
                });
            }
            Err(e) => {
                let error_msg = format!("Node #{}: {}", idx, e);
                warn!("[get_nodes] {}", error_msg);
                parse_errors.push(error_msg);
            }
        }
    }

    // 如果有解析错误，记录到日志但不影响返回有效节点
    if !parse_errors.is_empty() {
        warn!("[get_nodes] Skipped {} invalid node(s)", parse_errors.len());
    }

    success("Nodes loaded", nodes)
}

pub async fn add_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NodeRequest>,
) -> HandlerResult {
    Validator::validate_node_request(&req).map_err(|e| status_error(StatusCode::BAD_REQUEST, e))?;

    let node_type = req.node_type.as_deref().unwrap_or("hysteria2");

    // 验证节点类型是否支持
    const VALID_NODE_TYPES: &[&str] = &["hysteria2", "anytls", "ss", "trojan", "vmess", "vless", "tuic"];
    if !VALID_NODE_TYPES.contains(&node_type) {
        return Err(status_error(
            StatusCode::BAD_REQUEST,
            format!(
                "不支持的节点类型: {}，支持的类型: {}",
                node_type,
                VALID_NODE_TYPES.join(", ")
            ),
        ));
    }

    // VMess 需要验证 security 字段
    if node_type == "vmess" {
        if let Some(ref cipher) = req.cipher {
            if !VALID_VMESS_SECURITY.contains(&cipher.as_str()) {
                return Err(status_error(
                    StatusCode::BAD_REQUEST,
                    format!("不支持的 VMess 加密方式: {}", cipher),
                ));
            }
        }
    }

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();

    // 检查标签唯一性（大小写不敏感）
    let req_tag_lower = req.tag.to_lowercase();
    for node_str in &new_config.nodes {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(node_str) {
            if let Some(existing_tag) = v.get("tag").and_then(|t| t.as_str()) {
                if existing_tag.to_lowercase() == req_tag_lower {
                    return Err(status_error(
                        StatusCode::BAD_REQUEST,
                        format!(
                            "标签 '{}' 与已存在的节点 '{}' 重复（不区分大小写）",
                            req.tag, existing_tag
                        ),
                    ));
                }
            }
        }
    }

    let node_json = match node_type {
        "anytls" => {
            let node = AnyTls {
                outbound_type: "anytls".to_string(),
                tag: req.tag,
                server: req.server,
                server_port: req.server_port,
                password: req.password,
                tls: Tls {
                    enabled: true,
                    server_name: req.sni,
                    insecure: req.skip_cert_verify,
                },
            };
            serde_json::to_string(&node)
        }
        "ss" => {
            let node = Shadowsocks {
                outbound_type: "shadowsocks".to_string(),
                tag: req.tag,
                server: req.server,
                server_port: req.server_port,
                method: req
                    .cipher
                    .unwrap_or_else(|| "2022-blake3-aes-128-gcm".to_string()),
                password: req.password,
            };
            serde_json::to_string(&node)
        }
        "trojan" => {
            let node = Trojan {
                outbound_type: "trojan".to_string(),
                tag: req.tag,
                server: req.server,
                server_port: req.server_port,
                password: req.password,
                tls: Tls {
                    enabled: true,
                    server_name: req.sni,
                    insecure: req.skip_cert_verify,
                },
            };
            serde_json::to_string(&node)
        }
        "vmess" => {
            let node = VMess {
                outbound_type: "vmess".to_string(),
                tag: req.tag,
                server: req.server,
                server_port: req.server_port,
                uuid: req.password,
                security: req.cipher,
                alter_id: None,
                tls: if req.skip_cert_verify || req.sni.is_some() {
                    Some(Tls {
                        enabled: true,
                        server_name: req.sni,
                        insecure: req.skip_cert_verify,
                    })
                } else {
                    None
                },
            };
            serde_json::to_string(&node)
        }
        "vless" => {
            let node = Vless {
                outbound_type: "vless".to_string(),
                tag: req.tag,
                server: req.server,
                server_port: req.server_port,
                uuid: req.password,
                flow: req.cipher,
                tls: Tls {
                    enabled: true,
                    server_name: req.sni,
                    insecure: req.skip_cert_verify,
                },
            };
            serde_json::to_string(&node)
        }
        "tuic" => {
            let node = Tuic {
                outbound_type: "tuic".to_string(),
                tag: req.tag,
                server: req.server,
                server_port: req.server_port,
                uuid: req.password,
                token: None,
                congestion_control: req.cipher,
                tls: Tls {
                    enabled: true,
                    server_name: req.sni,
                    insecure: req.skip_cert_verify,
                },
            };
            serde_json::to_string(&node)
        }
        _ => {
            let node = Hysteria2 {
                outbound_type: "hysteria2".to_string(),
                tag: req.tag,
                server: req.server,
                server_port: req.server_port,
                password: req.password,
                up_mbps: None,
                down_mbps: None,
                tls: Tls {
                    enabled: true,
                    server_name: req.sni,
                    insecure: req.skip_cert_verify,
                },
            };
            serde_json::to_string(&node)
        }
    }
    .map_err(|e| {
        status_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize node: {}", e),
        )
    })?;

    new_config.nodes.push(node_json);

    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success_no_data("Node added and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn delete_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteNodeRequest>,
) -> HandlerResult {
    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();

    let original_len = new_config.nodes.len();
    new_config.nodes.retain(|node_str| {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(node_str) {
            v.get("tag").and_then(|t| t.as_str()) != Some(&req.tag)
        } else {
            true
        }
    });

    if new_config.nodes.len() == original_len {
        return Err(status_error(StatusCode::NOT_FOUND, "Node not found"));
    }

    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success_no_data("Node deleted and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, response::Json};

    use super::get_nodes;
    use crate::{models::Config, test_support::app_state};

    #[tokio::test]
    async fn get_nodes_returns_parsed_manual_nodes() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"node-a","server":"a.example.com","server_port":443,"password":"secret","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"server_name":"sni.example.com","insecure":true}}"#.to_string(),
                "not-json".to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "Nodes loaded");
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag, "node-a");
        assert_eq!(nodes[0].server, "a.example.com");
        assert_eq!(nodes[0].server_port, 443);
        assert_eq!(nodes[0].node_type, "hysteria2");
        assert_eq!(nodes[0].sni.as_deref(), Some("sni.example.com"));
    }

    #[tokio::test]
    async fn get_nodes_handles_hysteria2_without_bandwidth() {
        // 测试：Hysteria2 节点不包含带宽默认值也能被正确解析
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                // 不包含 up_mbps/down_mbps 的 Hysteria2 节点
                r#"{"type":"hysteria2","tag":"no-bw-node","server":"example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "Nodes loaded");
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag, "no-bw-node");
        assert_eq!(nodes[0].node_type, "hysteria2");
    }

    #[tokio::test]
    async fn get_nodes_skips_invalid_nodes_and_returns_valid_ones() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                // Valid node
                r#"{"type":"hysteria2","tag":"valid-node","server":"valid.example.com","server_port":443,"password":"secret"}"#.to_string(),
                // Invalid: missing tag
                r#"{"type":"hysteria2","server":"invalid1.example.com","server_port":443,"password":"secret"}"#.to_string(),
                // Invalid: zero port
                r#"{"type":"hysteria2","tag":"invalid-port","server":"invalid2.example.com","server_port":0,"password":"secret"}"#.to_string(),
                // Invalid: missing server
                r#"{"type":"hysteria2","tag":"invalid-server","server_port":443,"password":"secret"}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag, "valid-node");
    }

    #[tokio::test]
    async fn get_nodes_returns_empty_for_no_nodes() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "Nodes loaded");
        let nodes = response.data.unwrap();
        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn get_nodes_handles_all_invalid_nodes() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                "not-json".to_string(),
                r#"{}"#.to_string(),
                r#"{"type":"hysteria2"}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn get_nodes_handles_mixed_node_types() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"hy2-node","server":"hy2.example.com","server_port":443,"password":"secret"}"#.to_string(),
                r#"{"type":"shadowsocks","tag":"ss-node","server":"ss.example.com","server_port":8388,"password":"secret","method":"aes-128-gcm"}"#.to_string(),
                r#"{"type":"anytls","tag":"anytls-node","server":"any.example.com","server_port":8443,"password":"secret"}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 3);

        let types: Vec<String> = nodes.iter().map(|n| n.node_type.clone()).collect();
        assert!(types.contains(&"hysteria2".to_string()));
        assert!(types.contains(&"shadowsocks".to_string()));
        assert!(types.contains(&"anytls".to_string()));
    }

    #[tokio::test]
    async fn get_nodes_preserves_node_order() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"first","server":"first.example.com","server_port":443,"password":"secret"}"#.to_string(),
                r#"{"type":"hysteria2","tag":"second","server":"second.example.com","server_port":443,"password":"secret"}"#.to_string(),
                r#"{"type":"hysteria2","tag":"third","server":"third.example.com","server_port":443,"password":"secret"}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].tag, "first");
        assert_eq!(nodes[1].tag, "second");
        assert_eq!(nodes[2].tag, "third");
    }

    #[tokio::test]
    async fn get_nodes_handles_ipv6_addresses() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"ipv6-node","server":"2001:db8::1","server_port":443,"password":"secret"}"#.to_string(),
                r#"{"type":"hysteria2","tag":"localhost-ipv6","server":"::1","server_port":443,"password":"secret"}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].server, "2001:db8::1");
        assert_eq!(nodes[1].server, "::1");
    }

    #[tokio::test]
    async fn get_nodes_handles_unicode_tags() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"香港节点","server":"hk.example.com","server_port":443,"password":"secret"}"#.to_string(),
                r#"{"type":"hysteria2","tag":"日本サーバー","server":"jp.example.com","server_port":443,"password":"secret"}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].tag, "香港节点");
        assert_eq!(nodes[1].tag, "日本サーバー");
    }

    #[tokio::test]
    async fn get_nodes_handles_trojan_and_vmess() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"trojan","tag":"tr-node","server":"tr.example.com","server_port":443,"password":"secret","tls":{"enabled":true,"server_name":"tr.example.com","insecure":false}}"#.to_string(),
                r#"{"type":"vmess","tag":"vm-node","server":"vm.example.com","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","security":"auto","alter_id":0,"tls":{"enabled":true,"server_name":"vm.example.com","insecure":false}}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 2);

        let types: Vec<String> = nodes.iter().map(|n| n.node_type.clone()).collect();
        assert!(types.contains(&"trojan".to_string()));
        assert!(types.contains(&"vmess".to_string()));
    }

    #[tokio::test]
    async fn get_nodes_handles_vless_and_tuic() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"vless","tag":"vl-node","server":"vl.example.com","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","flow":"xtls-rprx-vision","tls":{"enabled":true,"server_name":"vl.example.com","insecure":false}}"#.to_string(),
                r#"{"type":"tuic","tag":"tu-node","server":"tu.example.com","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","congestion_control":"bbr","tls":{"enabled":true,"server_name":"tu.example.com","insecure":false}}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 2);

        let types: Vec<String> = nodes.iter().map(|n| n.node_type.clone()).collect();
        assert!(types.contains(&"vless".to_string()));
        assert!(types.contains(&"tuic".to_string()));
    }

    #[tokio::test]
    async fn get_nodes_preserves_sni_for_all_protocols() {
        let state = app_state(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"trojan","tag":"tr","server":"s","server_port":443,"password":"p","tls":{"enabled":true,"server_name":"sni-trojan","insecure":false}}"#.to_string(),
                r#"{"type":"vless","tag":"vl","server":"s","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","tls":{"enabled":true,"server_name":"sni-vless","insecure":false}}"#.to_string(),
                r#"{"type":"vmess","tag":"vm","server":"s","server_port":443,"uuid":"bf000d23-0752-40b4-affe-68f7707a9661","tls":{"enabled":true,"server_name":"sni-vmess","insecure":false}}"#.to_string(),
            ],
            custom_rules: vec![],
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 3);

        let snis: Vec<Option<String>> = nodes.iter().map(|n| n.sni.clone()).collect();
        assert_eq!(snis[0], Some("sni-trojan".to_string()));
        assert_eq!(snis[1], Some("sni-vless".to_string()));
        assert_eq!(snis[2], Some("sni-vmess".to_string()));
    }
}
