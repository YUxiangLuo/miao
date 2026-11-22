use std::{os::unix::fs::PermissionsExt, sync::Arc};

use axum::{routing::get, Router};
use tokio::sync::RwLock;

use crate::{
    models::{Config, RouteMode, StableConfig},
    services::config::{
        apply_route_mode, read_sub_nodes_snapshot, refresh_subscriptions_foreground,
    },
    state::AppState,
    test_support::isolated_stopped_state,
};

#[tokio::test]
async fn failed_snapshot_write_cannot_resurrect_removed_nodes_on_local_edit_or_restart() {
    snapshot_write_failure("proxies: []\n", None).await;
}

#[tokio::test]
async fn failed_snapshot_write_cannot_restore_old_credentials_on_local_edit_or_restart() {
    snapshot_write_failure(
        "proxies:\n- {name: subscription-node, type: trojan, server: 192.0.2.2, port: 443, password: new-password}\n",
        Some("new-password"),
    )
    .await;
}

async fn snapshot_write_failure(next_body: &str, expected_password: Option<&str>) {
    let body = Arc::new(RwLock::new(
        "proxies:\n- {name: subscription-node, type: trojan, server: 192.0.2.2, port: 443, password: old-password}\n".to_string(),
    ));
    let serving = body.clone();
    let app = Router::new().route(
        "/sub",
        get(move || {
            let serving = serving.clone();
            async move { serving.read().await.clone() }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/sub", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_root, state) = isolated_stopped_state(Config {
        subs: vec![url],
        nodes: vec![r#"{"type":"trojan","tag":"manual","server":"192.0.2.1","server_port":443,"password":"test"}"#.into()],
        ..Config::default()
    });
    let kernel = state.runtime_paths.runtime_dir.join("sing-box");
    std::fs::write(&kernel, "#!/bin/sh\n[ \"$1\" = check ]\n").unwrap();
    std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        refresh_subscriptions_foreground(&state)
            .await
            .unwrap()
            .fetch_succeeded
    );
    let old_snapshot = std::fs::read(&state.runtime_paths.sub_nodes_snapshot).unwrap();

    // Only the derived snapshot fails; all transactional files remain writable.
    std::fs::remove_file(&state.runtime_paths.sub_nodes_snapshot).unwrap();
    std::fs::create_dir(&state.runtime_paths.sub_nodes_snapshot).unwrap();
    *body.write().await = next_body.to_string();
    assert!(
        refresh_subscriptions_foreground(&state)
            .await
            .unwrap()
            .fetch_succeeded
    );
    assert_runtime_password(&state, expected_password).await;
    apply_route_mode(&state, RouteMode::Global).await.unwrap();
    assert_runtime_password(&state, expected_password).await;

    // Model a failed replacement that left the previous file readable. A new
    // AppState must not accept it simply because the subscription URLs match.
    std::fs::remove_dir(&state.runtime_paths.sub_nodes_snapshot).unwrap();
    std::fs::write(&state.runtime_paths.sub_nodes_snapshot, old_snapshot).unwrap();
    let config = state.config.read().await.clone();
    let restarted = Arc::new(
        AppState::with_config_layers(
            StableConfig::from(&config),
            config,
            state.config_path.clone(),
            state.volatile_path.clone(),
            state.runtime_paths.clone(),
        )
        .unwrap(),
    );
    restarted.lifecycle.request_running(false);
    assert!(
        read_sub_nodes_snapshot(&restarted).await.is_none(),
        "stale disk snapshot must be rejected"
    );
    let before_edit = std::fs::read(&state.runtime_paths.active_config).unwrap();
    let result = apply_route_mode(&restarted, RouteMode::Rule).await;
    // Missing authoritative material may reject an edit; it must never restore
    // old subscription credentials. Manual-only runtimes can still be rebuilt.
    if result.is_err() {
        assert_eq!(
            std::fs::read(&state.runtime_paths.active_config).unwrap(),
            before_edit
        );
    }
    assert_runtime_password(&restarted, expected_password).await;
    server.abort();
}

async fn assert_runtime_password(state: &AppState, expected: Option<&str>) {
    let runtime: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(&state.runtime_paths.active_config)
            .await
            .unwrap(),
    )
    .unwrap();
    let actual = runtime["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["server"] == "192.0.2.2")
        .and_then(|node| node["password"].as_str());
    assert_eq!(actual, expected);
}
