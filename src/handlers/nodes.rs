use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::Arc;

use crate::models::{AnyTls, ApiResponse, DeleteNodeRequest, Hysteria2, NodeInfo, NodeRequest, Shadowsocks, Tls};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::{regenerate_and_restart, save_config};
use crate::state::AppState;
use crate::validation::Validator;

pub async fn get_nodes(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<NodeInfo>>> {
    let config = state.config.lock().await;

    let nodes: Vec<NodeInfo> = config
        .nodes
        .iter()
        .filter_map(|s| {
            serde_json::from_str::<serde_json::Value>(s).ok().map(|v| NodeInfo {
                tag: v.get("tag").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                server: v.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                server_port: v.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0) as u16,
                node_type: v.get("type").and_then(|t| t.as_str()).unwrap_or("unknown").to_string(),
                sni: v
                    .get("tls")
                    .and_then(|t| t.get("server_name"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect();

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
        let mut config = state.config.lock().await;

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
                    up_mbps: 40,
                    down_mbps: 350,
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

        if let Err(e) = save_config(&config).await {
            return Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {}", e)));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone).await {
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
        let mut config = state.config.lock().await;

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

        if let Err(e) = save_config(&config).await {
            return Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {}", e)));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone).await {
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
}
