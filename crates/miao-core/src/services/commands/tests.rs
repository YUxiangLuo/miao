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
    let args = json!({"tag":"", "server":"", "server_port":0});
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

#[cfg(unix)]
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
}
