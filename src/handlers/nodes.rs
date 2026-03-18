use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::Arc;
use tracing::warn;

use crate::models::{AnyTls, ApiResponse, DeleteNodeRequest, Hysteria2, NodeInfo, NodeRequest, Shadowsocks, Tls};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::{regenerate_and_restart, save_config};
use crate::services::node_parser::parse_node_json;
use crate::state::AppState;
use crate::validation::Validator;

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
    Validator::validate_node_request(&req)
        .map_err(|e| status_error(StatusCode::BAD_REQUEST, e))?;

    let config_clone;
    {
        let mut config = state.config.write().await;

        for node_str in &config.nodes {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(node_str) {
                if v.get("tag").and_then(|t| t.as_str()) == Some(&req.tag) {
                    return Err(status_error(StatusCode::BAD_REQUEST, "Node with this tag already exists"));
                }
            }
        }

        let node_type = req.node_type.as_deref().unwrap_or("hysteria2");
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
                    method: req.cipher.unwrap_or_else(|| "2022-blake3-aes-128-gcm".to_string()),
                    password: req.password,
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
        }.map_err(|e| status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to serialize node: {}", e)))?;

        config.nodes.push(node_json);
        config_clone = config.clone();
    }

    if let Err(e) = save_config(&config_clone).await {
        return Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {}", e)));
    }

    match regenerate_and_restart(&config_clone, &state).await {
        Ok(_) => Ok(success_no_data("Node added and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn delete_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteNodeRequest>,
) -> HandlerResult {
    let config_clone;
    {
        let mut config = state.config.write().await;

        let original_len = config.nodes.len();
        config.nodes.retain(|node_str| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(node_str) {
                v.get("tag").and_then(|t| t.as_str()) != Some(&req.tag)
            } else {
                true
            }
        });

        if config.nodes.len() == original_len {
            return Err(status_error(StatusCode::NOT_FOUND, "Node not found"));
        }

        config_clone = config.clone();
    }

    if let Err(e) = save_config(&config_clone).await {
        return Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {}", e)));
    }

    match regenerate_and_restart(&config_clone, &state).await {
        Ok(_) => Ok(success_no_data("Node deleted and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, response::Json};

    use super::get_nodes;
    use crate::{
        models::Config,
        test_support::app_state,
    };

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
}
