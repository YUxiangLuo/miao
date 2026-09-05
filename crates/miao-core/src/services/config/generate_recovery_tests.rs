use super::*;
use crate::{
    models::{NodeSelect, Region},
    test_support::app_state,
};
use std::sync::atomic::AtomicU8;

#[tokio::test]
async fn failed_sources_keep_nodes_but_successful_empty_sources_replace_them() {
    use axum::{http::StatusCode, routing::get, Router};
    let mode = Arc::new(AtomicU8::new(0));
    let flag = mode.clone();
    let yaml = |name: &str| {
        format!("proxies:\n  - name: {name}\n    type: trojan\n    server: {name}.example.com\n    port: 443\n    password: fixture\n")
    };
    let app = Router::new()
        .route("/a", get(move || async move { yaml("node-a") }))
        .route(
            "/b",
            get(move || {
                let flag = flag.clone();
                async move {
                    match flag.load(Ordering::Relaxed) {
                        1 => (StatusCode::BAD_GATEWAY, "temporary failure".to_string()),
                        2 => (StatusCode::OK, "proxies: []".to_string()),
                        3 => (
                            StatusCode::OK,
                            "proxies:\n  - name: node-b\n    type: unsupported-fixture\n"
                                .to_string(),
                        ),
                        4 => (
                            StatusCode::OK,
                            "Subscription temporarily unavailable".to_string(),
                        ),
                        _ => (StatusCode::OK, yaml("node-b")),
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut config = Config {
        subs: vec![format!("{base}/a"), format!("{base}/b")],
        ..Config::default()
    };
    let state = app_state(config.clone());
    let first = gen_config(&config, &state, SubFetchRetry::None)
        .await
        .unwrap();
    crate::services::config::bindings::save_node_bindings(&state, &first.node_bindings)
        .await
        .unwrap();
    record_fresh_snapshot(&config, &state, &first).await;

    config
        .custom_rules
        .push(r#"{"domain":"example.com","outbound":"node-b"}"#.to_string());
    mode.store(1, Ordering::Relaxed);
    let partial = gen_config(&config, &state, SubFetchRetry::None)
        .await
        .unwrap();
    assert!(partial.has_sub_nodes);
    assert!(partial.skipped_rules.is_empty());
    assert_eq!(partial.fresh_sub_nodes.as_ref().unwrap().len(), 2);
    let failed_status = state.sub_status.lock().await[&config.subs[1]].clone();
    assert!(!failed_status.success);
    assert_eq!(failed_status.node_count, 1);
    record_fresh_snapshot(&config, &state, &partial).await;

    for failure in [3, 4] {
        mode.store(failure, Ordering::Relaxed);
        let malformed = gen_config(&config, &state, SubFetchRetry::None)
            .await
            .unwrap();
        assert!(
            malformed.skipped_rules.is_empty(),
            "unparseable nodes are not an intentional empty list"
        );
        assert_eq!(malformed.fresh_sub_nodes.as_ref().unwrap().len(), 2);
    }

    // A failed subscription explicitly removed by the user must not reappear.
    config.subs.pop();
    let removed = gen_config(&config, &state, SubFetchRetry::None)
        .await
        .unwrap();
    assert_eq!(removed.fresh_sub_nodes.as_ref().unwrap().len(), 1);
    config.subs.push(format!("{base}/b"));

    mode.store(2, Ordering::Relaxed);
    let empty = gen_config(&config, &state, SubFetchRetry::None)
        .await
        .unwrap();
    assert_eq!(empty.skipped_rules.len(), 1);
    record_fresh_snapshot(&config, &state, &empty).await;
    let local = gen_config_from_snapshot(&config, &state).await.unwrap();
    assert_eq!(
        local.skipped_rules.len(),
        1,
        "local edits must not resurrect removed nodes"
    );
    server.abort();
}

#[tokio::test]
async fn region_uses_current_name_while_rules_keep_the_stable_tag() {
    let mut config = Config {
        node_select: NodeSelect::Fastest(Region::Hk),
        ..Config::default()
    };
    let state = app_state(config.clone());
    let mut node = FetchedNode {
        source_id: subscription_source_id("https://fixture/sub"),
        name: "香港节点".to_string(),
        outbound: serde_json::json!({"type":"trojan","server":"same.example.com","server_port":443,"password":"fixture"}),
    };
    let first = build_prepared(&config, &state, vec![node.clone()])
        .await
        .unwrap();
    crate::services::config::bindings::save_node_bindings(&state, &first.node_bindings)
        .await
        .unwrap();
    node.name = "日本节点".to_string();
    config.node_select = NodeSelect::Fastest(Region::Jp);
    config
        .custom_rules
        .push(r#"{"domain":"example.com","outbound":"香港节点"}"#.to_string());
    let renamed = build_prepared(&config, &state, vec![node.clone()])
        .await
        .unwrap();
    assert_eq!(renamed.node_select, config.node_select);
    assert!(renamed.skipped_rules.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&renamed.bytes).unwrap();
    assert_eq!(
        json["outbounds"][0]["outbounds"],
        serde_json::json!(["香港节点"])
    );
    config.node_select = NodeSelect::Fastest(Region::Hk);
    assert_eq!(
        build_prepared(&config, &state, vec![node])
            .await
            .unwrap()
            .node_select,
        NodeSelect::Manual
    );
}

#[tokio::test]
async fn local_edits_without_a_snapshot_never_wait_for_subscription_network() {
    let config = Config {
        subs: vec!["http://127.0.0.1:1/must-not-fetch".to_string()],
        nodes: vec![r#"{"type":"trojan","tag":"manual","server":"example.com","server_port":443,"password":"fixture"}"#.to_string()],
        ..Config::default()
    };
    let state = app_state(config.clone());
    let manual_only = gen_config_from_snapshot(&config, &state).await.unwrap();
    assert!(!manual_only.has_sub_nodes);
    assert!(
        state.sub_status.lock().await.is_empty(),
        "local rebuilding must not launch a fetch"
    );

    tokio::fs::create_dir_all(&state.runtime_paths.runtime_dir)
        .await
        .unwrap();
    let previous = br#"{"outbounds":[{"tag":"subscription-node","type":"trojan"}]}"#;
    tokio::fs::write(&state.runtime_paths.active_config, previous)
        .await
        .unwrap();
    assert!(
        gen_config_from_snapshot(&config, &state).await.is_err(),
        "unknown subscription nodes must not be silently dropped"
    );
    assert_eq!(
        tokio::fs::read(&state.runtime_paths.active_config)
            .await
            .unwrap(),
        previous
    );
    assert!(state.sub_status.lock().await.is_empty());
}

#[tokio::test]
async fn snapshot_reads_share_a_projection_and_writes_replace_it() {
    let state = app_state(Config::default());
    let first =
        SubNodesSnapshot::from_fetched_nodes(vec!["https://fixture".to_string()], Vec::new());
    save_sub_nodes_snapshot(&state, &first).await.unwrap();
    let read_a = read_sub_nodes_snapshot(&state).await.unwrap();
    let read_b = read_sub_nodes_snapshot(&state).await.unwrap();
    assert!(Arc::ptr_eq(&read_a, &read_b));
    let second = SubNodesSnapshot::from_fetched_nodes(Vec::new(), Vec::new());
    save_sub_nodes_snapshot(&state, &second).await.unwrap();
    let read_c = read_sub_nodes_snapshot(&state).await.unwrap();
    assert!(!Arc::ptr_eq(&read_a, &read_c));
    assert!(read_c.subs.is_empty());
    assert_eq!(
        read_a.subs.len(),
        1,
        "readers retain a coherent old projection"
    );
}
