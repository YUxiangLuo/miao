use super::*;
use crate::models::{Config, RuntimePhase, SubscriptionFetchOutcome, SubscriptionRefreshPhase};
use crate::services::{config, singbox};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test]
async fn empty_response_without_a_replacement_keeps_runtime_without_network_retries() {
    use axum::{routing::get, Router};
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let app = Router::new().route("/sub", get(move || {
        let first = requests.fetch_add(1, Ordering::Relaxed) == 0;
        async move { if first {
            "proxies:\n  - name: remote\n    type: trojan\n    server: 192.0.2.1\n    port: 443\n    password: fixture\n"
        } else { "proxies: []" } }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = Config {
        subs: vec![format!("http://{}/sub", listener.local_addr().unwrap())],
        ..Config::default()
    };
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (state, root) = local_startup_test_state(cfg.clone(), "empty-no-replacement").await;
    assert!(!initialize_runtime_locked(&cfg, &state).await);
    assert!(super::super::startup::recover_data_plane_once(&state).await);
    let pid = singbox::kernel_status(&state).await.pid;
    let old_bytes = tokio::fs::read(&state.runtime_paths.active_config)
        .await
        .unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        super::super::refresh_subscriptions_in_background(&cfg, &state),
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(singbox::kernel_status(&state).await.pid, pid);
    assert_eq!(
        tokio::fs::read(&state.runtime_paths.active_config)
            .await
            .unwrap(),
        old_bytes
    );
    assert_eq!(
        state.subscription_refresh.snapshot().outcome,
        SubscriptionFetchOutcome::Empty
    );
    assert_eq!(
        state.config_warning.lock().await.as_deref(),
        Some(config::SUBS_NO_USABLE_KEEP_CACHE)
    );
    assert_eq!(
        config::read_sub_nodes_snapshot(&state)
            .await
            .unwrap()
            .to_fetched_nodes()
            .len(),
        1,
        "an unactivated candidate must not publish a new runtime snapshot"
    );
    singbox::stop_sing_internal(&state).await;
    server.abort();
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn successful_empty_subscription_finishes_recovery_and_commits_an_empty_snapshot() {
    use axum::{routing::get, Router};
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let app = Router::new().route(
        "/sub",
        get(move || {
            requests.fetch_add(1, Ordering::Relaxed);
            async { "proxies: []" }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = Config {
        subs: vec![format!("http://{}/sub", listener.local_addr().unwrap())],
        nodes: vec![serde_json::json!({"type":"trojan","tag":"manual","server":"192.0.2.1","server_port":443,"password":"fixture"}).to_string()],
        ..Config::default()
    };
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (state, root) = local_startup_test_state(cfg.clone(), "empty-background").await;
    assert!(initialize_runtime_locked(&cfg, &state).await);
    let pid = singbox::kernel_status(&state).await.pid;
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        super::super::refresh_subscriptions_in_background(&cfg, &state),
    )
    .await
    .expect("successful empty lists must not be retried as network failures");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(state.runtime_phase(), RuntimePhase::Ready);
    assert_eq!(singbox::kernel_status(&state).await.pid, pid);
    let activity = state.subscription_refresh.snapshot();
    assert_eq!(activity.phase, SubscriptionRefreshPhase::Completed);
    assert_eq!(activity.outcome, SubscriptionFetchOutcome::Empty);
    assert!(state.config_warning.lock().await.is_none());
    let snapshot = config::read_sub_nodes_snapshot(&state).await.unwrap();
    assert!(snapshot.to_fetched_nodes().is_empty());
    assert!(snapshot.matches_subs(&cfg.subs));

    // A foreground empty response is also a success, but only after commit.
    config::refresh_subscriptions_foreground(&state)
        .await
        .unwrap();
    let generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    assert!(generation > 0);
    assert_eq!(
        state.sub_refresh_success_generation.load(Ordering::Relaxed),
        generation
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(singbox::kernel_status(&state).await.pid, pid);
    singbox::stop_sing_internal(&state).await;
    server.abort();
    let _ = tokio::fs::remove_dir_all(root).await;
}
