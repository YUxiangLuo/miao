use axum::{extract::State, http::StatusCode, response::Json};
use serde_json::{json, Map, Value as JsonValue};
use std::sync::Arc;
use tracing::warn;

use crate::models::{ApiResponse, DeleteNodeRequest, NodeInfo, NodeRequest};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::apply_config_change;
use crate::services::node_parser::parse_node_json;
use crate::state::AppState;
use crate::validation::Validator;

const VALID_NODE_TYPES: &[&str] = &[
    "hysteria2",
    "anytls",
    "ss",
    "vmess",
    "vless",
    "trojan",
    "tuic",
];

fn non_empty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn insert_optional_string(obj: &mut Map<String, JsonValue>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        obj.insert(key.to_string(), json!(value));
    }
}

fn base_outbound(typ: &str, req: &NodeRequest) -> Map<String, JsonValue> {
    let mut obj = Map::new();
    obj.insert("type".to_string(), json!(typ));
    obj.insert("tag".to_string(), json!(req.tag.trim()));
    obj.insert("server".to_string(), json!(req.server.trim()));
    obj.insert("server_port".to_string(), json!(req.server_port));
    obj
}

fn build_tls(req: &NodeRequest, default_enabled: bool, force_enabled: bool) -> Option<JsonValue> {
    let enabled = force_enabled
        || req.tls_enabled.unwrap_or(default_enabled)
        || non_empty(&req.reality_public_key).is_some();
    if !enabled {
        return None;
    }

    let mut tls = Map::new();
    tls.insert("enabled".to_string(), json!(true));
    tls.insert("insecure".to_string(), json!(req.skip_cert_verify));
    insert_optional_string(&mut tls, "server_name", non_empty(&req.sni));

    if let Some(alpn) = req.alpn.as_ref().map(|values| {
        values
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    }) {
        if !alpn.is_empty() {
            tls.insert("alpn".to_string(), json!(alpn));
        }
    }

    if let Some(fingerprint) = non_empty(&req.client_fingerprint) {
        let fingerprint = fingerprint.to_ascii_lowercase();
        if fingerprint != "none" {
            tls.insert(
                "utls".to_string(),
                json!({
                    "enabled": true,
                    "fingerprint": fingerprint
                }),
            );
        }
    }

    if let Some(public_key) = non_empty(&req.reality_public_key) {
        let mut reality = Map::new();
        reality.insert("enabled".to_string(), json!(true));
        reality.insert("public_key".to_string(), json!(public_key));
        insert_optional_string(&mut reality, "short_id", non_empty(&req.reality_short_id));
        tls.insert("reality".to_string(), JsonValue::Object(reality));
    }

    Some(JsonValue::Object(tls))
}

fn build_transport(req: &NodeRequest) -> Option<JsonValue> {
    let transport_type = non_empty(&req.transport_type).unwrap_or("tcp");
    match transport_type {
        "tcp" => None,
        "ws" => {
            let mut transport = Map::new();
            transport.insert("type".to_string(), json!("ws"));
            insert_optional_string(&mut transport, "path", non_empty(&req.transport_path));
            if let Some(host) = non_empty(&req.transport_host) {
                transport.insert("headers".to_string(), json!({ "Host": host }));
            }
            Some(JsonValue::Object(transport))
        }
        "grpc" => {
            let mut transport = Map::new();
            transport.insert("type".to_string(), json!("grpc"));
            insert_optional_string(
                &mut transport,
                "service_name",
                non_empty(&req.grpc_service_name),
            );
            Some(JsonValue::Object(transport))
        }
        "http" | "h2" => {
            let mut transport = Map::new();
            transport.insert("type".to_string(), json!("http"));
            insert_optional_string(&mut transport, "path", non_empty(&req.transport_path));
            if let Some(host) = non_empty(&req.transport_host) {
                transport.insert("host".to_string(), json!([host]));
            }
            Some(JsonValue::Object(transport))
        }
        _ => None,
    }
}

fn build_node_value(req: &NodeRequest, node_type: &str) -> JsonValue {
    match node_type {
        "anytls" => {
            let mut obj = base_outbound("anytls", req);
            obj.insert("password".to_string(), json!(req.password.trim()));
            obj.insert(
                "tls".to_string(),
                build_tls(req, true, true).expect("AnyTLS TLS is always enabled"),
            );
            JsonValue::Object(obj)
        }
        "ss" => {
            let mut obj = base_outbound("shadowsocks", req);
            obj.insert(
                "method".to_string(),
                json!(non_empty(&req.cipher).unwrap_or("2022-blake3-aes-128-gcm")),
            );
            obj.insert("password".to_string(), json!(req.password.trim()));
            JsonValue::Object(obj)
        }
        "vmess" => {
            let mut obj = base_outbound("vmess", req);
            obj.insert(
                "uuid".to_string(),
                json!(non_empty(&req.uuid).unwrap_or_default()),
            );
            obj.insert(
                "security".to_string(),
                json!(non_empty(&req.cipher).unwrap_or("auto")),
            );
            obj.insert("alter_id".to_string(), json!(req.alter_id.unwrap_or(0)));
            insert_optional_string(&mut obj, "packet_encoding", non_empty(&req.packet_encoding));
            if let Some(tls) = build_tls(req, false, false) {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_transport(req) {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "vless" => {
            let mut obj = base_outbound("vless", req);
            obj.insert(
                "uuid".to_string(),
                json!(non_empty(&req.uuid).unwrap_or_default()),
            );
            insert_optional_string(&mut obj, "flow", non_empty(&req.flow));
            insert_optional_string(&mut obj, "packet_encoding", non_empty(&req.packet_encoding));
            if let Some(tls) = build_tls(req, true, false) {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_transport(req) {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "trojan" => {
            let mut obj = base_outbound("trojan", req);
            obj.insert("password".to_string(), json!(req.password.trim()));
            if let Some(tls) = build_tls(req, true, true) {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_transport(req) {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "tuic" => {
            let mut obj = base_outbound("tuic", req);
            obj.insert(
                "uuid".to_string(),
                json!(non_empty(&req.uuid).unwrap_or_default()),
            );
            obj.insert("password".to_string(), json!(req.password.trim()));
            obj.insert(
                "congestion_control".to_string(),
                json!(non_empty(&req.tuic_congestion_control).unwrap_or("cubic")),
            );
            obj.insert(
                "udp_relay_mode".to_string(),
                json!(non_empty(&req.tuic_udp_relay_mode).unwrap_or("native")),
            );
            if req.tuic_zero_rtt {
                obj.insert("zero_rtt_handshake".to_string(), json!(true));
            }
            obj.insert(
                "tls".to_string(),
                build_tls(req, true, true).expect("TUIC TLS is always enabled"),
            );
            JsonValue::Object(obj)
        }
        _ => {
            let mut obj = base_outbound("hysteria2", req);
            obj.insert("password".to_string(), json!(req.password.trim()));
            obj.insert(
                "tls".to_string(),
                build_tls(req, true, true).expect("Hysteria2 TLS is always enabled"),
            );
            if let Some(obfs_type) = non_empty(&req.obfs_type) {
                obj.insert(
                    "obfs".to_string(),
                    json!({
                        "type": obfs_type,
                        "password": non_empty(&req.obfs_password).unwrap_or_default()
                    }),
                );
            }
            JsonValue::Object(obj)
        }
    }
}

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

    let node_value = build_node_value(&req, node_type);
    let node_json = serde_json::to_string(&node_value).map_err(|e| {
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

    use super::{build_node_value, get_nodes};
    use crate::{
        models::{Config, NodeRequest},
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
            vps_ip: None,
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

    #[test]
    fn build_node_value_maps_manual_vmess_transport_and_tls() {
        let req = NodeRequest {
            node_type: Some("vmess".to_string()),
            tag: "vmess".to_string(),
            server: "vm.example.com".to_string(),
            server_port: 443,
            uuid: Some("123e4567-e89b-12d3-a456-426614174000".to_string()),
            cipher: Some("auto".to_string()),
            alter_id: Some(0),
            tls_enabled: Some(true),
            sni: Some("vm.example.com".to_string()),
            client_fingerprint: Some("chrome".to_string()),
            transport_type: Some("ws".to_string()),
            transport_path: Some("/ws".to_string()),
            transport_host: Some("cdn.example.com".to_string()),
            packet_encoding: Some("xudp".to_string()),
            ..NodeRequest::default()
        };

        let value = build_node_value(&req, "vmess");

        assert_eq!(value["type"], "vmess");
        assert_eq!(value["uuid"], "123e4567-e89b-12d3-a456-426614174000");
        assert_eq!(value["security"], "auto");
        assert_eq!(value["tls"]["server_name"], "vm.example.com");
        assert_eq!(value["tls"]["utls"]["fingerprint"], "chrome");
        assert_eq!(value["transport"]["type"], "ws");
        assert_eq!(value["transport"]["path"], "/ws");
        assert_eq!(value["transport"]["headers"]["Host"], "cdn.example.com");
    }

    #[test]
    fn build_node_value_maps_manual_tuic_defaults() {
        let req = NodeRequest {
            node_type: Some("tuic".to_string()),
            tag: "tuic".to_string(),
            server: "tuic.example.com".to_string(),
            server_port: 443,
            uuid: Some("123e4567-e89b-12d3-a456-426614174000".to_string()),
            password: "password123".to_string(),
            ..NodeRequest::default()
        };

        let value = build_node_value(&req, "tuic");

        assert_eq!(value["type"], "tuic");
        assert_eq!(value["congestion_control"], "cubic");
        assert_eq!(value["udp_relay_mode"], "native");
        assert_eq!(value["tls"]["enabled"], true);
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
            vps_ip: None,
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
            vps_ip: None,
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
            vps_ip: None,
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
            vps_ip: None,
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
            vps_ip: None,
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
            vps_ip: None,
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
            vps_ip: None,
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
            vps_ip: None,
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].tag, "香港节点");
        assert_eq!(nodes[1].tag, "日本サーバー");
    }
}
