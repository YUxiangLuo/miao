use super::{handle, MCP_PROTOCOL_VERSION};
use crate::models::Config;
use crate::test_support::app_state;
use serde_json::{json, Value as JsonValue};
use std::sync::Arc;

fn state(config: Config) -> Arc<crate::state::AppState> {
    let state = app_state(config);
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    state
}

async fn call(state: &Arc<crate::state::AppState>, body: JsonValue) -> JsonValue {
    handle(state, body.to_string().as_bytes())
        .await
        .expect("request must produce a response")
}

#[test]
fn flat_node_pool_covers_outbounds_beyond_group_members() {
    let proxies = json!({
        "proxy": { "type": "URLTest", "all": ["香港 01"], "now": "香港 01" },
        "direct": { "type": "Direct" },
        "香港 01": { "type": "Trojan" },
        "德国 01": { "type": "Vmess" },
        "手动A": { "type": "Hysteria2" }
    });

    let pool = super::flat_node_pool(&proxies);

    assert_eq!(pool, vec!["德国 01", "手动A", "香港 01"]);
}

#[tokio::test]
async fn parse_error_returns_32700() {
    let response = handle(&state(Config::default()), b"not json".as_slice())
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], -32700);
    assert_eq!(response["id"], JsonValue::Null);
}

#[tokio::test]
async fn notification_gets_no_response() {
    let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
    assert!(handle(
        &state(Config::default()),
        notification.to_string().as_bytes()
    )
    .await
    .is_none());
}

#[tokio::test]
async fn unknown_method_returns_32601() {
    let response = call(
        &state(Config::default()),
        json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
    )
    .await;
    assert_eq!(response["error"]["code"], -32601);
}

#[tokio::test]
async fn discover_reports_protocol_version_and_tools_capability() {
    for method in ["initialize", "server/discover"] {
        let response = call(
            &state(Config::default()),
            json!({ "jsonrpc": "2.0", "id": 1, "method": method }),
        )
        .await;
        let result = &response["result"];
        assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(result["serverInfo"]["name"], "miao");
        assert!(result["capabilities"]["tools"].is_object());
        // 调用者须知：流量可能经过本代理，破坏性操作不能自行确认
        let instructions = result["instructions"].as_str().unwrap();
        assert!(instructions.contains("流量很可能正经过它"));
        assert!(instructions.contains("绝不能自行把 confirm 设为 true"));
        assert!(instructions.contains("敏感信息"));
    }
}

#[tokio::test]
async fn tools_list_covers_panel_capabilities() {
    let response = call(
        &state(Config::default()),
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
    )
    .await;
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    let expected = [
        "get_status",
        "get_version_info",
        "start_service",
        "stop_service",
        "list_subscriptions",
        "add_subscriptions",
        "delete_subscription",
        "refresh_subscriptions",
        "scan_clash_verge",
        "list_subscription_nodes",
        "set_subscription_node_disabled",
        "list_nodes",
        "list_manual_nodes",
        "add_node",
        "import_nodes",
        "delete_node",
        "switch_node",
        "set_node_select",
        "set_max_multiplier",
        "test_delay",
        "set_route_mode",
        "list_rules",
        "add_rule",
        "delete_rule",
        "get_traffic",
        "list_connections",
        "test_connectivity",
        "set_mcp_enabled",
        "deploy_vps",
        "upgrade_miao",
    ];
    assert_eq!(names, expected);

    for tool in tools {
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.chars().count() >= 25,
            "description too short: {description}"
        );
        assert!(tool["inputSchema"].is_object());
        assert!(tool["annotations"]["readOnlyHint"].is_boolean());
        assert!(tool["annotations"]["destructiveHint"].is_boolean());
    }
    for name in [
        "stop_service",
        "delete_subscription",
        "delete_node",
        "delete_rule",
        "set_mcp_enabled",
        "deploy_vps",
        "upgrade_miao",
    ] {
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert!(tool["description"].as_str().unwrap().contains("确认"));
        assert_eq!(tool["inputSchema"]["properties"]["confirm"]["const"], true);
        assert_eq!(tool["annotations"]["destructiveHint"], true);
    }
    assert!(response["result"]["ttlMs"].is_number());
}

#[tokio::test]
async fn unknown_tool_returns_32602_style_error() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "explode", "arguments": {} },
        }),
    )
    .await;
    // 未知工具走 isError 结果（MCP 惯例），不是协议错误
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Unknown tool"));
}

#[tokio::test]
async fn get_status_works_when_stopped_without_network() {
    let selected = crate::models::NodeMultiplier::parse("2.5").unwrap();
    let state = state(Config {
        max_multiplier: Some(selected),
        ..Config::default()
    });
    *state.node_select_preference.write().await =
        crate::models::NodeSelect::Fastest(crate::models::Region::Jp);
    *state.available_multipliers.write().await = vec![
        crate::models::NodeMultiplier::ONE,
        selected,
        crate::models::NodeMultiplier::parse("6.5").unwrap(),
    ];
    let response = call(
        &state,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "get_status", "arguments": {} },
        }),
    )
    .await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: JsonValue = serde_json::from_str(text).unwrap();
    assert_eq!(payload["running"], false);
    assert_eq!(payload["ready"], false);
    assert_eq!(payload["route_mode"], "rule");
    assert_eq!(payload["node_select"], "manual");
    assert_eq!(payload["requested_node_select"], "fastest_jp");
    assert_eq!(payload["max_multiplier"], "2.5");
    assert_eq!(payload["multiplier_options"], json!(["1", "2.5", "6.5"]));
    assert_eq!(payload["mcp"], false);
    assert_eq!(
        payload["upgrade_supported"],
        crate::platform::upgrade_supported()
    );
    assert_eq!(payload["vps_supported"], crate::platform::vps_supported());
    assert!(payload["current_node"].is_null());
}

#[tokio::test]
async fn list_nodes_falls_back_to_manual_nodes_when_stopped() {
    let config = Config {
            nodes: vec![
                r#"{"type":"hysteria2","tag":"手动节点A","server":"a.example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#.to_string(),
            ],
            ..Default::default()
        };
    let response = call(
        &state(config),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list_nodes", "arguments": {} },
        }),
    )
    .await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: JsonValue = serde_json::from_str(text).unwrap();
    assert_eq!(payload["running"], false);
    let nodes = payload["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["name"], "手动节点A");
    assert_eq!(nodes[0]["source"], "manual");
    assert_eq!(nodes[0]["is_current"], false);
}

#[tokio::test]
async fn switch_node_rejects_fastest_mode() {
    let config = Config {
        node_select: crate::models::NodeSelect::Fastest(crate::models::Region::Hk),
        ..Default::default()
    };
    let response = call(
        &state(config),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "switch_node", "arguments": { "name": "香港-01" } },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("最快模式"));
}

#[tokio::test]
async fn switch_node_requires_running_service() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "switch_node", "arguments": { "name": "任意节点" } },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("服务未运行"));
}

#[tokio::test]
async fn switch_node_validates_arguments() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "switch_node", "arguments": {} },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("missing `name`"));
}

#[tokio::test]
async fn set_route_mode_validates_mode() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "set_route_mode", "arguments": { "mode": "moon" } },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("rule 或 global"));
}

#[tokio::test]
async fn set_node_select_validates_select_value() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "set_node_select", "arguments": { "select": "fastest_kr" } },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("manual"));
}

#[tokio::test]
async fn set_max_multiplier_validates_value() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "set_max_multiplier", "arguments": { "max_multiplier": "free" } },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("大于 0"));
}

#[tokio::test]
async fn set_max_multiplier_is_idempotent_without_touching_runtime() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "set_max_multiplier", "arguments": { "max_multiplier": null } },
        }),
    )
    .await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: JsonValue = serde_json::from_str(text).unwrap();
    assert_eq!(payload["max_multiplier"], JsonValue::Null);
    assert_eq!(payload["changed"], false);
    assert_eq!(payload["note"], "未变化");
}

#[tokio::test]
async fn set_node_select_is_idempotent_without_touching_runtime() {
    // 默认 manual，请求 manual：未变化直接返回，不起内核不写盘
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "set_node_select", "arguments": { "select": "manual" } },
        }),
    )
    .await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: JsonValue = serde_json::from_str(text).unwrap();
    assert_eq!(payload["node_select"], "manual");
    assert_eq!(payload["changed"], false);
    assert_eq!(payload["note"], "未变化");
}

#[tokio::test]
async fn set_route_mode_is_idempotent_without_touching_runtime() {
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "set_route_mode", "arguments": { "mode": "rule" } },
        }),
    )
    .await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: JsonValue = serde_json::from_str(text).unwrap();
    assert_eq!(payload["route_mode"], "rule");
    assert_eq!(payload["changed"], false);
    assert_eq!(payload["note"], "未变化");
    assert_eq!(payload["runtime_updated"], false);
}

#[tokio::test]
async fn refresh_subscriptions_requires_subs() {
    // 无订阅：在进管线前报错（不触网不起内核）
    let response = call(
        &state(Config::default()),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "refresh_subscriptions", "arguments": {} },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("没有配置订阅"));
}

#[tokio::test]
async fn list_rules_returns_structured_entries() {
    let config = Config {
        custom_rules: vec![
            r#"{"process_name":"curl","action":"route","outbound":"direct"}"#.to_string(),
        ],
        ..Default::default()
    };
    let response = call(
        &state(config),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list_rules", "arguments": {} },
        }),
    )
    .await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: JsonValue = serde_json::from_str(text).unwrap();
    let rules = payload["rules"].as_array().unwrap();
    assert_eq!(rules[0]["field"], "process_name");
    assert_eq!(rules[0]["value"], "curl");
    assert_eq!(rules[0]["target"], "direct");
    assert_eq!(rules[0]["skipped"], false);
}

#[tokio::test]
async fn list_subscriptions_matches_panel_data() {
    let config = Config {
        subs: vec!["https://example.com/sub?token=secret".to_string()],
        ..Default::default()
    };
    let response = call(
        &state(config),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list_subscriptions", "arguments": {} },
        }),
    )
    .await;
    let payload: JsonValue =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        payload["subscriptions"][0]["url"],
        "https://example.com/sub?token=secret"
    );
    assert_eq!(payload["subscriptions"][0]["state"], "pending");
}

#[tokio::test]
async fn list_subscription_nodes_matches_panel_data() {
    let mut config = Config {
        subs: vec!["https://example.com/sub".to_string()],
        ..Default::default()
    };
    config.disabled_nodes = vec![crate::models::DisabledNode {
        sub: "https://example.com/sub".to_string(),
        name: "node-1".to_string(),
    }];
    let state = state(config);
    let snapshot = crate::services::config::SubNodesSnapshot {
        version: 1,
        subs: vec!["https://example.com/sub".to_string()],
        node_names: vec!["node-1".to_string(), "node-2".to_string()],
        outbounds: vec![
            serde_json::json!({"type":"trojan","server":"a.example.com","server_port":443}),
            serde_json::json!({"type":"vless","server":"b.example.com","server_port":8443}),
        ],
        source_ids: vec![
            crate::services::config::subscription_source_id("https://example.com/sub"),
            crate::services::config::subscription_source_id("https://example.com/sub"),
        ],
    };
    crate::services::config::save_sub_nodes_snapshot(&state, &snapshot)
        .await
        .unwrap();

    let response = call(
        &state,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list_subscription_nodes", "arguments": {} },
        }),
    )
    .await;
    let payload: JsonValue =
        serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let nodes = &payload["subscriptions"][0]["nodes"];
    assert_eq!(nodes[0]["name"], "node-1");
    assert_eq!(nodes[0]["disabled"], true);
    assert_eq!(nodes[1]["disabled"], false);
    assert_eq!(nodes[1]["server"], "b.example.com");
}

#[tokio::test]
async fn set_subscription_node_disabled_validates_params() {
    let state = state(Config::default());
    // 缺 disabled 字段 → 参数错误
    let response = call(
            &state,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "set_subscription_node_disabled", "arguments": { "sub": "https://example.com/sub", "name": "n" } },
            }),
        )
        .await;
    let text = response["error"]["message"]
        .as_str()
        .unwrap_or("")
        .to_string()
        + response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("");
    assert!(text.contains("Invalid params"), "unexpected: {text}");

    // 订阅不存在 → handler 校验错误
    let response = call(
            &state,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "set_subscription_node_disabled", "arguments": { "sub": "https://example.com/nope", "name": "n", "disabled": true } },
            }),
        )
        .await;
    let text = response["error"]["message"]
        .as_str()
        .unwrap_or("")
        .to_string()
        + response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("");
    assert!(text.contains("订阅不存在"), "unexpected: {text}");
}

#[tokio::test]
async fn list_manual_nodes_matches_panel_data_without_secrets() {
    let config = Config {
            nodes: vec![
                r#"{"type":"hysteria2","tag":"手动A","server":"a.example.com","server_port":443,"password":"secret","tls":{"enabled":true,"server_name":"sni.example.com"}}"#.to_string(),
            ],
            ..Default::default()
        };
    let response = call(
        &state(config),
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": "list_manual_nodes", "arguments": {} },
        }),
    )
    .await;
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    let payload: JsonValue = serde_json::from_str(text).unwrap();
    assert_eq!(payload["nodes"][0]["tag"], "手动A");
    assert_eq!(payload["nodes"][0]["server"], "a.example.com");
    assert_eq!(payload["nodes"][0]["sni"], "sni.example.com");
    assert!(!text.contains("secret"));
}

#[tokio::test]
async fn destructive_tools_require_explicit_confirmation_before_side_effects() {
    let state = state(Config::default());
    assert!(state
        .service_should_run
        .load(std::sync::atomic::Ordering::Relaxed));
    for name in ["stop_service", "set_mcp_enabled", "upgrade_miao"] {
        let response = call(
            &state,
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": name, "arguments": {} },
            }),
        )
        .await;
        assert_eq!(response["result"]["isError"], true, "tool {name}");
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("明确确认"));
    }
    assert!(state
        .service_should_run
        .load(std::sync::atomic::Ordering::Relaxed));
    assert!(!state.config.read().await.mcp);
}

#[tokio::test]
async fn add_subscriptions_reuses_panel_validation_without_network() {
    let state = state(Config::default());
    let response = call(
        &state,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "add_subscriptions",
                "arguments": { "urls": ["ftp://example.com/not-supported"] }
            },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(state.config.read().await.subs.is_empty());
}

#[tokio::test]
async fn add_node_reuses_panel_protocol_validation() {
    let state = state(Config::default());
    let response = call(
        &state,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {
                "name": "add_node",
                "arguments": {
                    "node_type": "hysteria2",
                    "tag": "missing-password",
                    "server": "example.com",
                    "server_port": 443
                }
            },
        }),
    )
    .await;
    assert_eq!(response["result"]["isError"], true);
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("密码"));
    assert!(state.config.read().await.nodes.is_empty());
}

#[test]
fn connection_pagination_validates_bounds() {
    assert_eq!(
        super::pagination_value(&json!({}), "limit", 100, 1, Some(500)).unwrap(),
        100
    );
    assert_eq!(
        super::pagination_value(&json!({ "limit": 500 }), "limit", 100, 1, Some(500)).unwrap(),
        500
    );
    assert!(super::pagination_value(&json!({ "limit": 0 }), "limit", 100, 1, Some(500)).is_err());
    assert!(super::pagination_value(&json!({ "limit": 501 }), "limit", 100, 1, Some(500)).is_err());
}
