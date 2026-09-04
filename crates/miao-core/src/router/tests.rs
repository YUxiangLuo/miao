use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use super::build_router;
use crate::{
    models::Config,
    test_support::{
        app_state, empty_request, json_request, response_json, response_text, test_app,
    },
};

#[tokio::test]
async fn router_serves_index_page() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app.oneshot(empty_request("GET", "/")).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_text(response).await;
    assert!(body.contains("Miao 控制面板"));
}

#[tokio::test]
async fn router_serves_favicon_with_svg_content_type() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(empty_request("GET", "/favicon.svg"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "image/svg+xml"
    );
    let body = response_text(response).await;
    assert!(body.contains("<svg"));
}

#[tokio::test]
async fn router_returns_status_payload() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(empty_request("GET", "/api/status"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "stopped");
    assert_eq!(json["data"]["running"], false);
    assert_eq!(json["data"]["requested_node_select"], "manual");
    assert_eq!(json["data"]["max_multiplier"], serde_json::Value::Null);
    assert_eq!(json["data"]["multiplier_options"], json!([]));
    assert_eq!(json["data"]["warnings"], json!([]));
    assert_eq!(
        json["data"]["vps_supported"],
        crate::platform::vps_supported()
    );
    assert_eq!(
        json["data"]["platform"],
        if cfg!(windows) { "windows" } else { "linux" }
    );
}

#[tokio::test]
async fn router_returns_version_capability_flags() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(empty_request("GET", "/api/version"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(
        json["data"]["upgrade_supported"],
        crate::platform::upgrade_supported()
    );
    assert_eq!(json["data"]["has_update"], false);
}

#[tokio::test]
async fn router_returns_node_list_payload() {
    let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"router-node","server":"node.example.com","server_port":443,"password":"secret","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"server_name":"sni.example.com","insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        })
        .await;

    let response = app
        .oneshot(empty_request("GET", "/api/nodes"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "Nodes loaded");
    assert_eq!(json["data"][0]["tag"], "router-node");
    assert_eq!(json["data"][0]["server"], "node.example.com");
    assert_eq!(json["data"][0]["sni"], "sni.example.com");
}

#[tokio::test]
async fn router_returns_subscription_list_payload() {
    let app = test_app(Config {
        port: None,
        subs: vec!["https://example.com/subscription".to_string()],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(empty_request("GET", "/api/subs"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "Subscriptions loaded");
    assert_eq!(json["data"][0]["url"], "https://example.com/subscription");
    assert_eq!(json["data"][0]["node_count"], 0);
}

#[tokio::test]
async fn router_rejects_config_mutation_during_initialization() {
    let state = app_state(Config::default());
    let app = build_router(state);

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/subs",
            json!({ "url": "https://example.com/subscription" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["message"], "Initialization is still in progress");
}

#[tokio::test]
async fn router_rejects_duplicate_subscription_with_bad_request() {
    let app = test_app(Config {
        port: None,
        subs: vec!["https://example.com/subscription".to_string()],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/subs",
            json!({ "url": "https://example.com/subscription" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["message"], "Subscription already exists");
}

#[tokio::test]
async fn router_returns_not_found_when_deleting_missing_subscription() {
    let app = test_app(Config {
        port: None,
        subs: vec!["https://example.com/subscription".to_string()],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(json_request(
            "DELETE",
            "/api/subs",
            json!({ "url": "https://example.com/missing" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["message"], "Subscription not found");
}

#[tokio::test]
async fn router_rejects_duplicate_node_with_bad_request() {
    let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"router-node","server":"node.example.com","server_port":443,"password":"password123","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        })
        .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/nodes",
            json!({
                "tag": "router-node",
                "server": "node.example.com",
                "server_port": 443,
                "password": "password123"
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert!(json["message"].as_str().unwrap().contains("重复"));
}

#[tokio::test]
async fn router_batch_import_reports_per_item_validation_failures() {
    let app = test_app(Config::default()).await;
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/nodes/import",
            json!({
                "nodes": [
                    {
                        "tag": "",
                        "server": "bad address",
                        "server_port": 0,
                        "password": ""
                    },
                    {
                        "node_type": "unsupported",
                        "tag": "node-b",
                        "server": "b.example.com",
                        "server_port": 443,
                        "password": "password123"
                    }
                ]
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["data"]["added"], json!([]));
    assert_eq!(json["data"]["failed"].as_array().unwrap().len(), 2);
    assert_eq!(json["data"]["failed"][0]["index"], 0);
    assert_eq!(json["data"]["failed"][1]["index"], 1);
}

#[tokio::test]
async fn router_returns_not_found_when_deleting_missing_node() {
    let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"router-node","server":"node.example.com","server_port":443,"password":"secret","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        })
        .await;

    let response = app
        .oneshot(json_request(
            "DELETE",
            "/api/nodes",
            json!({ "tag": "missing-node" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["message"], "Node not found");
}

#[tokio::test]
async fn router_rejects_invalid_rule_field_with_bad_request() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/rules",
            json!({ "field": "rule_set", "value": "x", "target": "proxy" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("不支持的规则字段"));
}

#[tokio::test]
async fn router_returns_not_found_when_deleting_missing_rule() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(json_request(
            "DELETE",
            "/api/rules",
            json!({ "index": 3, "raw": "anything" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["message"], "Rule not found");
}

#[tokio::test]
async fn router_rejects_rule_delete_when_entry_moved() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![
            r#"{"process_name":"curl","action":"route","outbound":"direct"}"#.to_string(),
        ],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(json_request(
            "DELETE",
            "/api/rules",
            json!({ "index": 0, "raw": r#"{"domain_suffix":"example.com"}"# }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert!(json["message"].as_str().unwrap().contains("规则列表已变化"));
}

#[tokio::test]
async fn router_returns_rule_list_payload() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![
            r#"{"process_name":"curl","action":"route","outbound":"direct"}"#.to_string(),
            r#"{"rule_set":["custom"],"action":"route","outbound":"proxy"}"#.to_string(),
        ],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(empty_request("GET", "/api/rules"))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["data"][0]["field"], "process_name");
    assert_eq!(json["data"][0]["value"], "curl");
    assert_eq!(json["data"][0]["target"], "direct");
    // 无法结构化识别的手写规则保留 raw
    assert!(json["data"][1]["field"].is_null());
    assert_eq!(
        json["data"][1]["raw"],
        r#"{"rule_set":["custom"],"action":"route","outbound":"proxy"}"#
    );
}

#[cfg(windows)]
#[tokio::test]
async fn router_omits_upgrade_and_vps_on_windows() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let upgrade = app
        .clone()
        .oneshot(empty_request("POST", "/api/upgrade"))
        .await
        .unwrap();
    assert_eq!(upgrade.status(), StatusCode::NOT_FOUND);

    let vps = app
        .oneshot(json_request(
            "POST",
            "/api/vps/deploy",
            json!({ "ip": "203.0.113.10", "password": "secret" }),
        ))
        .await
        .unwrap();
    assert_eq!(vps.status(), StatusCode::NOT_FOUND);
}

#[cfg(not(windows))]
#[tokio::test]
async fn router_rejects_vps_deploy_with_invalid_ip() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/vps/deploy",
            json!({ "ip": "bad ip", "password": "secret" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
}

#[cfg(not(windows))]
#[tokio::test]
async fn router_rejects_vps_deploy_with_empty_password() {
    let app = test_app(Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
    })
    .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/vps/deploy",
            json!({ "ip": "203.0.113.10", "password": "" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert_eq!(json["message"], "root 密码不能为空");
}

#[cfg(not(windows))]
#[tokio::test]
async fn router_vps_deploy_returns_existing_node_without_ssh() {
    let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"vps-node","server":"203.0.113.10","server_port":543,"password":"secret","tls":{"enabled":true,"insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
            max_multiplier: None,
            disabled_nodes: Default::default(),
        })
        .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/vps/deploy",
            json!({ "ip": "203.0.113.10", "password": "any" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["data"]["tag"], "vps-node");
    assert!(json["message"].as_str().unwrap().contains("已存在"));
}

#[tokio::test]
async fn router_returns_manual_node_select_by_default() {
    let app = test_app(Config::default()).await;
    let response = app
        .oneshot(empty_request("GET", "/api/status"))
        .await
        .unwrap();
    let json = response_json(response).await;
    assert_eq!(json["data"]["node_select"], "manual");
}

#[tokio::test]
async fn router_node_select_is_idempotent_when_state_matches() {
    let app = test_app(Config::default()).await;
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/node-select",
            json!({ "node_select": "manual" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["success"], true);
    assert_eq!(json["message"], "Node select unchanged");
}

#[tokio::test]
async fn router_updates_and_clears_max_multiplier() {
    let state = app_state(Config::default());
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    let app = build_router(state.clone());

    let response = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/max-multiplier",
            json!({ "max_multiplier": "2.5" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        state
            .config
            .read()
            .await
            .max_multiplier
            .map(|value| value.to_string()),
        Some("2.5".to_string())
    );

    let response = app
        .oneshot(json_request(
            "POST",
            "/api/max-multiplier",
            json!({ "max_multiplier": null }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(state.config.read().await.max_multiplier, None);
}

#[tokio::test]
async fn router_rejects_invalid_or_missing_max_multiplier() {
    for (body, expected) in [
        (
            json!({ "max_multiplier": "not-a-number" }),
            StatusCode::BAD_REQUEST,
        ),
        (json!({}), StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        let app = test_app(Config::default()).await;
        let response = app
            .oneshot(json_request("POST", "/api/max-multiplier", body))
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

#[tokio::test]
async fn router_rejects_unknown_node_select() {
    let app = test_app(Config::default()).await;
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/node-select",
            json!({ "node_select": "fastest_kr" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = response_json(response).await;
    assert_eq!(json["success"], false);
    assert!(json["message"]
        .as_str()
        .unwrap()
        .contains("不支持的节点选择"));
}

#[tokio::test]
async fn router_rejects_node_select_during_initialization() {
    let state = app_state(Config::default());
    let app = build_router(state);
    let response = app
        .oneshot(json_request(
            "POST",
            "/api/node-select",
            json!({ "node_select": "fastest_hk" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn mcp_endpoint_is_not_found_when_disabled() {
    let app = test_app(Config::default()).await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/mcp",
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mcp_endpoint_serves_jsonrpc_when_enabled() {
    let app = test_app(Config {
        mcp: true,
        node_select: Default::default(),
        max_multiplier: None,
        disabled_nodes: Default::default(),
        ..Default::default()
    })
    .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/mcp",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "router-test", "version": "1.0" }
                }
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("MCP-Protocol-Version").unwrap(),
        "2025-11-25"
    );
    let json = response_json(response).await;
    assert_eq!(json["result"]["protocolVersion"], "2025-11-25");
}

#[tokio::test]
async fn mcp_endpoint_accepts_initialized_notification() {
    let app = test_app(Config {
        mcp: true,
        ..Default::default()
    })
    .await;
    let mut request = json_request(
        "POST",
        "/mcp",
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );
    request.headers_mut().insert(
        "mcp-protocol-version",
        HeaderValue::from_static("2025-11-25"),
    );

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
}

#[tokio::test]
async fn mcp_endpoint_rejects_unsupported_protocol_header() {
    let app = test_app(Config {
        mcp: true,
        ..Default::default()
    })
    .await;
    let mut request = json_request(
        "POST",
        "/mcp",
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
    );
    request.headers_mut().insert(
        "mcp-protocol-version",
        HeaderValue::from_static("2026-07-28"),
    );

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = response_json(response).await;
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Unsupported"));
}

#[tokio::test]
async fn mcp_endpoint_requires_negotiated_protocol_after_initialize() {
    let app = test_app(Config {
        mcp: true,
        ..Default::default()
    })
    .await;

    let response = app
        .oneshot(json_request(
            "POST",
            "/mcp",
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mcp_get_returns_method_not_allowed_when_enabled() {
    let app = test_app(Config {
        mcp: true,
        ..Default::default()
    })
    .await;

    let response = app.oneshot(empty_request("GET", "/mcp")).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(response.headers().get("Allow").unwrap(), "POST");
}
