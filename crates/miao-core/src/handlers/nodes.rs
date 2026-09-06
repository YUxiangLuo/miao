use crate::models::{
    ApiResponse, BatchNodeRequest, BatchNodeResult, DeleteNodeRequest, NodeInfo, NodeRequest,
};
use crate::responses::{command_reply, command_result, HandlerResult};
use crate::services::commands;
use crate::state::AppState;
use axum::{extract::State, response::Json};
#[cfg(test)]
use commands::nodes::build_node_value;
use std::sync::Arc;

pub async fn get_nodes(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<NodeInfo>>> {
    command_reply(commands::nodes::get_nodes(state).await)
}

pub async fn add_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NodeRequest>,
) -> HandlerResult {
    command_result(commands::nodes::add_node(state, req).await)
}

pub async fn import_nodes(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchNodeRequest>,
) -> HandlerResult<BatchNodeResult> {
    command_result(commands::nodes::import_nodes(state, req).await)
}

pub async fn delete_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteNodeRequest>,
) -> HandlerResult {
    command_result(commands::nodes::delete_node(state, req).await)
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            password: Some("password123".to_string()),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        });

        let Json(response) = get_nodes(State(state)).await;

        assert!(response.success);
        let nodes = response.data.unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].tag, "香港节点");
        assert_eq!(nodes[1].tag, "日本サーバー");
    }
}
