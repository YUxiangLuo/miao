use super::*;
use crate::{models::*, test_support::isolated_stopped_state};
use axum::{extract::State, http::StatusCode, Json};
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};

async fn tool(state: &Arc<crate::state::AppState>, name: &str, args: Value) -> Value {
    let request = json!({"jsonrpc":"2.0", "id":1, "method":"tools/call",
        "params":{"name":name, "arguments":args}});
    crate::services::mcp::handle(state, request.to_string().as_bytes())
        .await
        .unwrap()
}

#[tokio::test]
async fn connectivity_distinguishes_http_response_from_request_failure() {
    let (_root, state) = isolated_stopped_state(Config::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new()
                .route(
                    "/ok",
                    axum::routing::head(|| async { StatusCode::NO_CONTENT }),
                )
                .route(
                    "/denied",
                    axum::routing::head(|| async { StatusCode::FORBIDDEN }),
                )
                .route(
                    "/slow",
                    axum::routing::head(|| async {
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        StatusCode::OK
                    }),
                ),
        )
        .await
        .unwrap();
    });
    for (path, expected) in [("ok", 204), ("denied", 403)] {
        let url = format!("http://{addr}/{path}");
        let result = service::test_connectivity(
            state.clone(),
            service::ConnectivityRequest { url: url.clone() },
        )
        .await
        .data
        .unwrap();
        assert!(result.success);
        assert_eq!(result.http_status, Some(expected));
        assert!(result.latency_ms.is_some());
        assert!(result.error.is_none());
        let mcp = tool(&state, "test_connectivity", json!({"url": url})).await;
        let text = mcp["result"]["content"][0]["text"].as_str().unwrap();
        let payload: Value = serde_json::from_str(text).unwrap();
        assert_eq!(payload["http_status"], expected);
    }
    let result = service::test_connectivity(
        state.clone(),
        service::ConnectivityRequest {
            url: format!("http://{addr}/slow"),
        },
    )
    .await
    .data
    .unwrap();
    assert!(!result.success);
    assert_eq!(result.error_kind.as_deref(), Some("timeout"));
    assert!(result.http_status.is_none());
    assert!(result.error.is_some());
    server.abort();
    let result = service::test_connectivity(
        state.clone(),
        service::ConnectivityRequest {
            url: "not a URL".into(),
        },
    )
    .await
    .data
    .unwrap();
    assert!(!result.success);
    assert_eq!(result.error_kind.as_deref(), Some("request"));
    assert!(result.http_status.is_none());
    assert!(!state.config_path.exists());
    assert!(state.sing_process.lock().await.is_none());
}

#[tokio::test]
async fn rest_mcp_and_commands_share_initialization_and_not_found_errors() {
    let (_root, state) = isolated_stopped_state(Config::default());
    for (initializing, expected_kind, expected_status) in [
        (true, CommandErrorKind::Conflict, StatusCode::CONFLICT),
        (false, CommandErrorKind::NotFound, StatusCode::NOT_FOUND),
    ] {
        state.initializing.store(initializing, Ordering::Relaxed);
        let command = nodes::delete_node(
            state.clone(),
            DeleteNodeRequest {
                tag: "missing".into(),
            },
        )
        .await
        .err()
        .unwrap();
        let (status, Json(rest)) = crate::handlers::nodes::delete_node(
            State(state.clone()),
            Json(DeleteNodeRequest {
                tag: "missing".into(),
            }),
        )
        .await
        .err()
        .unwrap();
        let mcp = tool(
            &state,
            "delete_node",
            json!({"tag":"missing", "confirm":true}),
        )
        .await;
        assert_eq!(command.kind, expected_kind);
        assert_eq!(status, expected_status);
        assert!(!rest.success);
        assert!(rest.data.is_none());
        assert_eq!(rest.message, command.message);
        assert_eq!(mcp["result"]["isError"], true);
        assert_eq!(mcp["result"]["content"][0]["text"], command.message);
        assert!(!state.config_path.exists());
        assert!(state.sing_process.lock().await.is_none());
    }
}

#[tokio::test]
async fn rest_and_mcp_retain_rule_conflict_and_invalid_node_validation() {
    let original = r#"{"domain_suffix":"example.com","action":"reject"}"#;
    let (_root, state) = isolated_stopped_state(Config {
        custom_rules: vec![original.into()],
        ..Config::default()
    });
    let request = DeleteRuleRequest {
        index: 0,
        raw: "outdated".into(),
    };
    let (status, Json(rest)) =
        crate::handlers::rules::delete_rule(State(state.clone()), Json(request))
            .await
            .err()
            .unwrap();
    let mcp = tool(
        &state,
        "delete_rule",
        json!({"index":0,"raw":"outdated","confirm":true}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(mcp["result"]["content"][0]["text"], rest.message);
    assert_eq!(state.config.read().await.custom_rules, [original]);

    let request = NodeRequest::default();
    // Keep the MCP payload schema-valid so both transports reach the shared
    // domain validator and can be compared on the same application error.
    let args = json!({"tag":"", "server":"", "server_port":443});
    let (status, Json(rest)) =
        crate::handlers::nodes::add_node(State(state.clone()), Json(request))
            .await
            .err()
            .unwrap();
    let mcp = tool(&state, "add_node", args).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(mcp["result"]["content"][0]["text"], rest.message);
    assert!(!state.config_path.exists());
}

#[tokio::test]
async fn rest_mcp_and_commands_publish_the_same_rule_read_model() {
    let (_root, state) = isolated_stopped_state(Config {
        custom_rules: vec![r#"{"domain_suffix":"example.com","action":"reject"}"#.into()],
        ..Config::default()
    });
    let command = rules::get_rules(state.clone()).await;
    let Json(rest) = crate::handlers::rules::get_rules(State(state.clone())).await;
    let mcp = tool(&state, "list_rules", json!({})).await;
    assert!(rest.success);
    assert_eq!(rest.message, command.message);
    assert_eq!(
        serde_json::to_value(&rest.data).unwrap(),
        serde_json::to_value(&command.data).unwrap()
    );
    assert_eq!(
        mcp["result"]["structuredContent"]["rules"],
        serde_json::to_value(rest.data).unwrap()
    );
}

#[tokio::test]
async fn mcp_setting_is_a_shared_persistence_only_command() {
    let (_command_root, command_state) = isolated_stopped_state(Config::default());
    let (_rest_root, rest_state) = isolated_stopped_state(Config::default());
    let (_mcp_root, mcp_state) = isolated_stopped_state(Config::default());
    let revision = command_state.data_revision.load(Ordering::Relaxed);
    for enabled in [true, true, false] {
        let command = settings::set_mcp(command_state.clone(), McpRequest { enabled })
            .await
            .unwrap();
        let Json(rest) =
            crate::handlers::mcp::set_mcp(State(rest_state.clone()), Json(McpRequest { enabled }))
                .await
                .ok()
                .unwrap();
        let mcp = tool(
            &mcp_state,
            "set_mcp_enabled",
            json!({"enabled":enabled,"confirm":true}),
        )
        .await;
        assert!(rest.success);
        assert_eq!(rest.message, command.message);
        assert_eq!(mcp["result"]["isError"], false);
        assert_eq!(
            mcp["result"]["structuredContent"]["message"],
            command.message
        );
        assert_eq!(mcp["result"]["structuredContent"]["data"], Value::Null);
        for state in [&command_state, &rest_state, &mcp_state] {
            assert_eq!(state.config.read().await.mcp, enabled);
            let persisted: Config =
                yaml_serde::from_slice(&tokio::fs::read(&state.config_path).await.unwrap())
                    .unwrap();
            assert_eq!(persisted.mcp, enabled);
            assert!(!state.runtime_paths.active_config.exists());
            assert!(state.sing_process.lock().await.is_none());
        }
    }
    assert_eq!(
        command_state.data_revision.load(Ordering::Relaxed),
        revision + 2
    );
}

#[tokio::test]
async fn concurrent_rest_and_mcp_node_adds_share_the_local_edit_lock() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, state) = isolated_stopped_state(Config::default());
    let kernel = state.runtime_paths.runtime_dir.join("sing-box");
    // Only validation succeeds. This binary cannot run a proxy or create a TUN.
    tokio::fs::write(&kernel, b"#!/bin/sh\n[ \"$1\" = check ]\n")
        .await
        .unwrap();
    std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();
    let request = NodeRequest {
        node_type: Some("trojan".into()),
        tag: "rest-node".into(),
        server: "127.0.0.1".into(),
        server_port: 443,
        password: Some("secret123".into()),
        ..NodeRequest::default()
    };
    let (rest, mcp) = tokio::join!(
        crate::handlers::nodes::add_node(State(state.clone()), Json(request)),
        tool(
            &state,
            "add_node",
            json!({"node_type":"trojan", "tag":"mcp-node",
            "server":"127.0.0.1", "server_port":443, "password":"secret123"})
        ),
    );
    let Json(rest) = rest.unwrap_or_else(|(_, Json(error))| panic!("{}", error.message));
    assert!(rest.success);
    assert_eq!(mcp["result"]["isError"], false, "{mcp}");
    let reply = nodes::get_nodes(state.clone()).await;
    let mut tags: Vec<_> = reply
        .data
        .unwrap()
        .into_iter()
        .map(|node| node.tag)
        .collect();
    tags.sort();
    assert_eq!(tags, ["mcp-node", "rest-node"]);
    let persisted: Config =
        yaml_serde::from_slice(&tokio::fs::read(&state.config_path).await.unwrap()).unwrap();
    assert_eq!(persisted.nodes.len(), 2);
    assert!(state.runtime_paths.active_config.is_file());
    assert!(state.sing_process.lock().await.is_none());
    assert!(!state.lifecycle.snapshot().should_run);
}

#[test]
fn application_operations_do_not_depend_on_http_handlers_or_axum() {
    for source in [
        include_str!("nodes.rs"),
        include_str!("subs.rs"),
        include_str!("rules.rs"),
        include_str!("service.rs"),
        include_str!("settings.rs"),
        include_str!("vps.rs"),
        include_str!("../mcp/panel.rs"),
        include_str!("../mcp.rs"),
    ] {
        assert!(!source.contains("crate::handlers"));
        assert!(!source.contains("axum::"));
        assert!(!source.contains("crate::responses"));
    }

    let mcp = include_str!("../mcp.rs");
    let panel = include_str!("../mcp/panel.rs");
    for operation in [
        "apply_route_mode",
        "apply_node_select",
        "apply_max_multiplier",
        "refresh_subscriptions_foreground",
    ] {
        assert!(
            !mcp.contains(operation),
            "MCP transport must use the shared command for {operation}"
        );
    }
    for command in [
        "commands::service::set_route_mode",
        "commands::service::set_node_select",
        "commands::service::set_max_multiplier",
        "commands::subs::refresh_subs",
    ] {
        assert!(panel.contains(command), "missing shared command: {command}");
    }
}

#[test]
fn subscription_refresh_result_preserves_the_refreshed_compatibility_field() {
    for fetch_succeeded in [false, true] {
        let result = subs::SubscriptionRefreshResult {
            refreshed: fetch_succeeded,
            fetch_succeeded,
            report: Default::default(),
            runtime_updated: false,
            started: false,
            reloaded: false,
            restarted: false,
            warning: None,
        };
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["refreshed"], fetch_succeeded);
        assert_eq!(value["refreshed"], value["fetch_succeeded"]);
    }
}

#[tokio::test]

async fn rest_subscription_refresh_retains_failed_fetch_details() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, state) = isolated_stopped_state(Config {
        subs: vec!["not a URL".into()],
        nodes: vec![json!({"type":"shadowsocks", "tag":"manual", "server":"127.0.0.1", "server_port":443, "method":"aes-128-gcm", "password":"demo"}).to_string()],
        ..Config::default()
    });
    let kernel = state.runtime_paths.runtime_dir.join("sing-box");
    std::fs::write(&kernel, "#!/bin/sh\n[ \"$1\" = check ]\n").unwrap();
    std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();
    let Json(reply) = crate::handlers::subs::refresh_subs(State(state.clone()))
        .await
        .unwrap_or_else(|(_, Json(error))| panic!("{}", error.message));
    assert!(
        reply.success,
        "the request completed, not the network fetch"
    );
    let result = reply.data.expect("REST must retain the fetch result");
    assert!(!result.fetch_succeeded);
    assert!(!result.refreshed);
    assert!(!result.runtime_updated);
    assert_eq!(result.report.failed_sources, 1);
    assert_eq!(result.report.successful_sources, 0);
    assert!(state.sing_process.lock().await.is_none());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new().route("/sub", axum::routing::get(|| async { "proxies: []" })),
        )
        .await
        .unwrap();
    });
    state.config.write().await.subs.extend([
        format!("http://{addr}/sub?a"),
        format!("http://{addr}/sub?b"),
    ]);
    let Json(reply) = crate::handlers::subs::refresh_subs(State(state.clone()))
        .await
        .unwrap_or_else(|(_, Json(error))| panic!("{}", error.message));
    server.abort();
    let result = reply.data.unwrap();
    assert!(result.fetch_succeeded);
    assert_eq!(result.report.successful_sources, 2);
    assert_eq!(result.report.failed_sources, 1);
    assert_eq!(result.report.fresh_nodes, 0);
    assert!(!result.runtime_updated);
}

#[tokio::test]
async fn scheduled_refresh_is_a_shared_persistence_only_command() {
    let (_command_root, command_state) = isolated_stopped_state(Config::default());
    let (_rest_root, rest_state) = isolated_stopped_state(Config::default());
    let (_mcp_root, mcp_state) = isolated_stopped_state(Config::default());
    let revision = command_state.data_revision.load(Ordering::Relaxed);

    let request = || ScheduledRefreshRequest {
        enabled: true,
        times: vec!["20:00".into(), "4:05".into(), "20:00".into()],
    };
    let command = settings::set_scheduled_refresh(command_state.clone(), request())
        .await
        .unwrap();
    let Json(rest) =
        crate::handlers::service::set_scheduled_refresh(State(rest_state.clone()), Json(request()))
            .await
            .ok()
            .unwrap();
    let mcp = tool(
        &mcp_state,
        "set_scheduled_refresh",
        json!({"enabled": true, "times": ["20:00", "4:05", "20:00"]}),
    )
    .await;

    assert!(rest.success);
    assert_eq!(rest.message, command.message);
    assert_eq!(mcp["result"]["isError"], false);
    assert_eq!(
        mcp["result"]["structuredContent"]["message"],
        command.message
    );
    // 三入口写入的稳定层完全一致：排序去重后的时刻 + 持久化 YAML
    for state in [&command_state, &rest_state, &mcp_state] {
        let stable = state.stable_config.read().await.scheduled_refresh.clone();
        assert!(stable.enabled);
        assert_eq!(stable.times, vec!["04:05", "20:00"]);
        let persisted: StableConfig =
            yaml_serde::from_slice(&tokio::fs::read(&state.config_path).await.unwrap()).unwrap();
        assert_eq!(persisted.scheduled_refresh, stable);
        assert!(!state.runtime_paths.active_config.exists());
        assert!(state.sing_process.lock().await.is_none());
    }
    assert_eq!(
        command_state.data_revision.load(Ordering::Relaxed),
        revision + 1
    );

    // 读取路径同样三入口一致，且带下次执行时间与时区信息
    let command_get = settings::get_scheduled_refresh(command_state.clone()).await;
    let Json(rest_get) =
        crate::handlers::service::get_scheduled_refresh(State(rest_state.clone())).await;
    let mcp_get = tool(&mcp_state, "get_scheduled_refresh", json!({})).await;
    let status = command_get.data.as_ref().unwrap();
    assert_eq!(status.times, vec!["04:05", "20:00"]);
    assert!(status.next_run_at.is_some());
    assert!(!status.utc_offset.is_empty());
    // `now` 每次调用都不同，逐字段比较两个入口的读数投影。
    let rest_status = rest_get.data.as_ref().unwrap();
    assert_eq!(rest_status.times, status.times);
    assert_eq!(rest_status.utc_offset, status.utc_offset);
    assert_eq!(rest_status.timezone, status.timezone);
    assert_eq!(rest_status.enabled, status.enabled);
    assert_eq!(
        mcp_get["result"]["structuredContent"]["times"],
        serde_json::to_value(&status.times).unwrap()
    );
    assert_eq!(
        mcp_get["result"]["structuredContent"]["utc_offset"],
        status.utc_offset
    );

    // 其它稳定层保存（如 MCP 开关）不能抹掉定时刷新设置
    settings::set_mcp(rest_state.clone(), McpRequest { enabled: true })
        .await
        .unwrap();
    let persisted: StableConfig =
        yaml_serde::from_slice(&tokio::fs::read(&rest_state.config_path).await.unwrap()).unwrap();
    assert!(persisted.scheduled_refresh.enabled);
    assert_eq!(persisted.scheduled_refresh.times, vec!["04:05", "20:00"]);

    // 校验失败：启用但没有时刻、非法格式；失败不得覆盖已保存设置
    let (_empty_status, Json(empty)) = crate::handlers::service::set_scheduled_refresh(
        State(rest_state.clone()),
        Json(ScheduledRefreshRequest {
            enabled: true,
            times: vec![],
        }),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(empty.message, "启用定时刷新时至少需要一个时间点");
    let (status_code, Json(invalid)) = crate::handlers::service::set_scheduled_refresh(
        State(rest_state.clone()),
        Json(ScheduledRefreshRequest {
            enabled: false,
            times: vec!["25:00".into()],
        }),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(status_code, StatusCode::BAD_REQUEST);
    assert!(invalid.message.contains("无效的时间"));
    assert!(
        rest_state
            .stable_config
            .read()
            .await
            .scheduled_refresh
            .enabled
    );
}
