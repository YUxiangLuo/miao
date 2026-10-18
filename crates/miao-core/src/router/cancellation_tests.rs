use std::{os::unix::fs::PermissionsExt, sync::Arc};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout, Duration},
};

use crate::{
    models::{Config, NodeSelect, Region, RuntimePhase},
    services::{
        config::{gen_config_from_nodes, install_prepared_runtime},
        singbox::{kernel_status, start_sing_internal, stop_sing_internal},
    },
    state::AppState,
    test_support::isolated_stopped_state,
};

async fn wait_for_phase(
    state: &AppState,
    phase: RuntimePhase,
) -> Result<(), tokio::time::error::Elapsed> {
    timeout(Duration::from_secs(3), async {
        while state.lifecycle.snapshot().phase != phase {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
}

async fn fixture() -> (tempfile::TempDir, Arc<AppState>) {
    let config = Config {
        mcp: true,
        node_select: NodeSelect::Fastest(Region::Tw),
        nodes: ["台湾节点", "日本节点"].iter().map(|tag| serde_json::json!({
            "type":"trojan", "tag":tag, "server":"192.0.2.1", "server_port":443, "password":"fixture"
        }).to_string()).collect(),
        ..Config::default()
    };
    let (root, state) = isolated_stopped_state(config.clone());
    let binary = state.runtime_paths.runtime_dir.join("sing-box");
    tokio::fs::write(&binary, b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\ntrap ':' HUP\nwhile :; do sleep 0.02; done\n").await.unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let generated = gen_config_from_nodes(&config, &state, vec![])
        .await
        .unwrap();
    install_prepared_runtime(&state, &generated).await.unwrap();
    state.lifecycle.request_running(true);
    start_sing_internal(&state).await.unwrap();
    (root, state)
}

#[tokio::test]
async fn disconnected_node_selection_finishes_reload_and_commits_state() {
    disconnected_selection(false, false).await;
}

#[tokio::test]
async fn disconnected_mcp_selection_finishes_reload_and_commits_state() {
    disconnected_selection(true, false).await;
}

#[tokio::test]
async fn disconnected_selection_still_rolls_back_a_failed_commit() {
    disconnected_selection(false, true).await;
}

async fn disconnected_selection(via_mcp: bool, fail_commit: bool) {
    let (_root, state) = fixture().await;
    let before = kernel_status(&state).await;
    let original_runtime = tokio::fs::read(&state.runtime_paths.active_config)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = super::build_router(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (path, body) = if via_mcp {
        ("/mcp", serde_json::json!({"jsonrpc":"2.0", "id":1,"method":"tools/call", "params":{"name":"set_node_select","arguments":{"select":"fastest_jp"}}}).to_string())
    } else {
        (
            "/api/node-select",
            r#"{"node_select":"fastest_jp"}"#.to_string(),
        )
    };
    let mut connection = TcpStream::connect(address).await.unwrap();
    connection.write_all(format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nMCP-Protocol-Version: {}\r\nContent-Length: {}\r\n\r\n{body}", crate::services::mcp::MCP_PROTOCOL_VERSION, body.len()
    ).as_bytes()).await.unwrap();
    wait_for_phase(&state, RuntimePhase::Reloading)
        .await
        .unwrap();
    if fail_commit {
        // Fail the durable commit after the candidate has reached the kernel.
        tokio::fs::create_dir(&state.config_path).await.unwrap();
    }
    drop(connection);

    let completed = wait_for_phase(&state, RuntimePhase::Ready).await;
    // Acquiring the transaction lock also waits for the persistent commit.
    let _guard = timeout(Duration::from_secs(3), state.config_update.lock())
        .await
        .unwrap();
    let status = kernel_status(&state).await;
    let config = state.config.read().await.clone();
    let requested = *state.node_select_preference.read().await;
    let runtime = tokio::fs::read(&state.runtime_paths.active_config)
        .await
        .unwrap();
    let preference = tokio::fs::read_to_string(&state.runtime_paths.node_select_preference)
        .await
        .ok();
    stop_sing_internal(&state).await;
    server.abort();
    let _ = server.await;

    assert!(
        completed.is_ok(),
        "disconnected request stranded kernel status: {status:?}"
    );
    assert_eq!(status.pid, before.pid);
    assert!(status.ready);
    if fail_commit {
        assert_eq!(config.node_select, NodeSelect::Fastest(Region::Tw));
        assert_eq!(runtime, original_runtime);
        assert!(
            preference.is_none(),
            "rollback restores the absent preference file"
        );
    } else {
        assert_eq!(config.node_select, NodeSelect::Fastest(Region::Jp));
        assert_eq!(preference.as_deref().map(str::trim), Some("fastest_jp"));
        assert_eq!(
            runtime,
            tokio::fs::read(&state.runtime_paths.config_cache)
                .await
                .unwrap()
        );
    }
    assert_eq!(requested, config.node_select);
}

#[tokio::test]
async fn shutdown_waits_for_disconnected_write_before_its_config_transaction() {
    shutdown_after_disconnect(WriteOutcome::Committed).await;
}

#[tokio::test]
async fn failed_disconnected_write_does_not_strand_shutdown() {
    shutdown_after_disconnect(WriteOutcome::Failed).await;
}

#[tokio::test]
async fn panicking_disconnected_write_does_not_strand_shutdown() {
    shutdown_after_disconnect(WriteOutcome::Panicked).await;
}

#[derive(Clone, Copy)]
enum WriteOutcome {
    Committed,
    Failed,
    Panicked,
}

async fn shutdown_after_disconnect(outcome: WriteOutcome) {
    let (_root, state) = isolated_stopped_state(Config {
        mcp: true,
        ..Default::default()
    });
    if matches!(outcome, WriteOutcome::Failed) {
        tokio::fs::create_dir(&state.config_path).await.unwrap();
    }
    let entered = Arc::new(tokio::sync::Notify::new());
    let resume = Arc::new(tokio::sync::Notify::new());
    let completed = Arc::new(tokio::sync::Notify::new());
    let (target, started, release, finished) = (
        state.clone(),
        entered.clone(),
        resume.clone(),
        completed.clone(),
    );
    // VPS/upgrade writes wait for external work before taking config_update.
    let app = axum::Router::new()
        .route(
            "/write",
            axum::routing::post(move || {
                let (target, started, release, finished) = (
                    target.clone(),
                    started.clone(),
                    release.clone(),
                    finished.clone(),
                );
                async move {
                    started.notify_one();
                    release.notified().await;
                    if matches!(outcome, WriteOutcome::Panicked) {
                        finished.notify_one();
                        panic!("injected write panic");
                    }
                    let response = crate::responses::command_result(
                        crate::services::commands::settings::set_mcp(
                            target,
                            crate::models::McpRequest { enabled: false },
                        )
                        .await,
                    );
                    finished.notify_one();
                    response
                }
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            super::finish_mutations_after_disconnect,
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let shutting_down = state.clone();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                stopped.await.unwrap();
                crate::runtime::shutdown_runtime(&shutting_down).await;
            })
            .await
            .unwrap();
    });
    let mut connection = TcpStream::connect(address).await.unwrap();
    connection
        .write_all(b"POST /write HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    timeout(Duration::from_secs(1), entered.notified())
        .await
        .unwrap();
    drop(connection);
    sleep(Duration::from_millis(30)).await;
    stop.send(()).unwrap();
    let exited_early = timeout(Duration::from_millis(100), &mut server)
        .await
        .is_ok();
    let before_release = state.config.read().await.mcp;
    resume.notify_one();
    timeout(Duration::from_secs(1), completed.notified())
        .await
        .unwrap();
    if !exited_early {
        timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
    }
    let after_release = state.config.read().await.mcp;
    assert!(
        !exited_early,
        "server reported shutdown complete while an accepted write was still waiting to commit"
    );
    assert!(before_release);
    assert_eq!(after_release, !matches!(outcome, WriteOutcome::Committed));
    if matches!(outcome, WriteOutcome::Committed) {
        let saved: crate::models::StableConfig =
            yaml_serde::from_str(&tokio::fs::read_to_string(&state.config_path).await.unwrap())
                .unwrap();
        assert!(!saved.mcp);
    }
    assert_eq!(state.lifecycle.snapshot().phase, RuntimePhase::Stopped);
}

#[tokio::test]
async fn shutdown_rejects_new_rest_and_mcp_writes_but_allows_status_reads() {
    use crate::test_support::{empty_request, json_request, response_json};
    use tower::ServiceExt;
    let (_root, state) = isolated_stopped_state(Config {
        mcp: true,
        ..Default::default()
    });
    crate::runtime::shutdown_runtime(&state).await;
    let app = super::build_router(state.clone());
    for uri in ["/api/mcp", "/mcp"] {
        let response = app
            .clone()
            .oneshot(json_request(
                "POST",
                uri,
                serde_json::json!({"enabled":false}),
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        let body = response_json(response).await;
        assert_eq!(body["success"], false);
        assert_eq!(body["message"], "Service is shutting down");
    }
    assert!(state.config.read().await.mcp);
    assert!(!state.config_path.exists());
    let response = app
        .oneshot(empty_request("GET", "/api/status"))
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(response_json(response).await["data"]["phase"], "stopped");
}

#[tokio::test]
async fn shutdown_cancels_incomplete_rest_body_before_stopping_kernel() {
    shutdown_with_incomplete_body("/api/mcp", false).await;
}

#[tokio::test]
async fn shutdown_cancels_incomplete_chunked_mcp_body_before_stopping_kernel() {
    shutdown_with_incomplete_body("/mcp", true).await;
}

async fn shutdown_with_incomplete_body(path: &str, chunked: bool) {
    let (_root, state) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = super::build_router(state.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let closing = state.clone();
    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                stopped.await.unwrap();
                crate::runtime::shutdown_runtime(&closing).await;
            })
            .await
            .unwrap();
    });
    let mut connection = TcpStream::connect(address).await.unwrap();
    let framing = if chunked {
        "Transfer-Encoding: chunked"
    } else {
        "Content-Length: 17"
    };
    connection.write_all(format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n{framing}\r\nExpect: 100-continue\r\n\r\n"
    ).as_bytes()).await.unwrap();
    // Hyper sends this only when the handler starts polling the request body.
    // This avoids a timing-only test that shuts down before the request arrives.
    let expected = b"HTTP/1.1 100 Continue\r\n\r\n";
    let mut interim = vec![0; expected.len()];
    timeout(Duration::from_secs(1), connection.read_exact(&mut interim))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(interim, expected);
    connection
        .write_all(if chunked { b"1\r\n{\r\n" } else { b"{" })
        .await
        .unwrap();
    stop.send(()).unwrap();
    // Keep the client open and never finish its body. Neither kernel shutdown
    // nor the HTTP server's graceful drain may depend on this peer cooperating.
    let finished = timeout(Duration::from_secs(1), &mut server).await;
    let status = kernel_status(&state).await;
    drop(connection);
    if finished.is_err() {
        timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
    }
    assert!(
        finished.is_ok(),
        "incomplete request blocked shutdown: {status:?}"
    );
    finished.unwrap().unwrap();
    assert!(!status.running);
    assert_eq!(status.phase, RuntimePhase::Stopped);
    assert!(state.config.read().await.mcp);
    assert!(!state.config_path.exists());
}

#[tokio::test(start_paused = true)]
async fn incomplete_body_times_out_without_starting_a_write() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    let (_root, state) = isolated_stopped_state(Config {
        mcp: true,
        ..Default::default()
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/mcp")
        .header("Content-Type", "application/json")
        .body(Body::from_stream(futures::stream::pending::<
            Result<axum::body::Bytes, std::io::Error>,
        >()))
        .unwrap();
    let response = timeout(
        super::REQUEST_BODY_TIMEOUT + Duration::from_secs(1),
        super::build_router(state.clone()).oneshot(request),
    )
    .await
    .expect("body reception must have its own deadline")
    .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::REQUEST_TIMEOUT);
    assert!(state.config.read().await.mcp);
    assert!(!state.config_path.exists());
    timeout(
        Duration::from_secs(1),
        crate::runtime::shutdown_runtime(&state),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn buffered_writes_preserve_default_and_overridden_body_limits() {
    use axum::{body::Body, extract::DefaultBodyLimit, http::Request};
    use tower::ServiceExt;

    let (_root, state) = isolated_stopped_state(Config {
        mcp: true,
        ..Default::default()
    });
    for limit in [None, Some(8)] {
        let mut app = super::build_router(state.clone());
        let mut body = r#"{"enabled":false}"#.to_string();
        if let Some(limit) = limit {
            app = app.layer(DefaultBodyLimit::max(limit));
        } else {
            body.push_str(&" ".repeat(2 * 1024 * 1024));
        }
        let request = Request::builder()
            .method("POST")
            .uri("/api/mcp")
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .unwrap();
        assert_eq!(
            app.oneshot(request).await.unwrap().status(),
            axum::http::StatusCode::PAYLOAD_TOO_LARGE
        );
    }
    assert!(state.config.read().await.mcp);
    assert!(!state.config_path.exists());
}

#[tokio::test]
async fn failed_body_reception_does_not_start_a_write() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    let (_root, state) = isolated_stopped_state(Config {
        mcp: true,
        ..Default::default()
    });
    let request = Request::builder()
        .method("POST")
        .uri("/api/mcp")
        .header("Content-Type", "application/json")
        .body(Body::from_stream(futures::stream::once(async {
            Err::<axum::body::Bytes, _>(std::io::Error::other("body interrupted"))
        })))
        .unwrap();
    let response = super::build_router(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    assert!(state.config.read().await.mcp);
    assert!(!state.config_path.exists());
    timeout(
        Duration::from_secs(1),
        crate::runtime::shutdown_runtime(&state),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn shutdown_as_body_finishes_still_rejects_the_write() {
    use axum::{
        body::{Body, Bytes},
        http::Request,
    };
    use tower::ServiceExt;

    let (_root, state) = isolated_stopped_state(Config {
        mcp: true,
        ..Default::default()
    });
    let closing = state.clone();
    let request = Request::builder()
        .method("POST")
        .uri("/api/mcp")
        .header("Content-Type", "application/json")
        .body(Body::from_stream(futures::stream::once(async move {
            closing.lifecycle.shutdown();
            Ok::<_, std::io::Error>(Bytes::from_static(br#"{"enabled":false}"#))
        })))
        .unwrap();
    let response = super::build_router(state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(state.config.read().await.mcp);
    assert!(!state.config_path.exists());
}
