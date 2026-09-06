use super::*;
use std::sync::{atomic::Ordering, Arc};

use crate::models::{Config, NodeSelect, Region, RuntimePhase};
use crate::services::{config::SUBS_RETRYING_SLOWLY, singbox};

// The test-only initial fetch has three requests, then four outer retries.
const FAST_REQUESTS: usize = 3 + crate::runtime::startup::STARTUP_BACKGROUND_FAST_RETRIES;

fn manual_config(subscription: String) -> Config {
    Config {
        subs: vec![subscription],
        nodes: vec![serde_json::json!({
            "type": "hysteria2",
            "tag": "manual-fallback",
            "server": "192.0.2.1",
            "server_port": 443,
            "password": "test"
        })
        .to_string()],
        node_select: NodeSelect::Fastest(Region::Jp),
        ..Config::default()
    }
}

async fn wait_for_slow_retry(state: &Arc<crate::state::AppState>) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if state.config_warning.lock().await.as_deref() == Some(SUBS_RETRYING_SLOWLY) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a usable manual runtime must eventually stop fast startup retries");
}

#[tokio::test]
async fn exhausted_startup_retries_preserve_manual_runtime_and_later_recover_quietly() {
    use axum::{http::StatusCode, routing::get, Router};

    let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Notify::new());
    let release_request = release.clone();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let request_calls = calls.clone();
    let app = Router::new().route(
        "/sub",
        get(move || {
            let release = release_request.clone();
            let requests = requests.clone();
            let calls = request_calls.clone();
            async move {
                let attempt = calls.fetch_add(1, Ordering::Relaxed) + 1;
                requests.send(attempt).unwrap();
                // Hold low-frequency requests so the test can inspect phase
                // while HTTP is in flight, rather than missing a brief flicker.
                if attempt > FAST_REQUESTS {
                    release.notified().await;
                }
                if attempt <= FAST_REQUESTS + 1 {
                    (StatusCode::SERVICE_UNAVAILABLE, "offline")
                } else {
                    (StatusCode::OK, "proxies:\n  - name: 日本-recovered\n    type: hysteria2\n    server: 127.0.0.1\n    port: 443\n    password: test\n")
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = manual_config(format!("http://{}/sub", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (state, root) = local_startup_test_state(config.clone(), "bounded-background").await;
    assert!(initialize_runtime_locked(&config, &state).await);
    let pid = singbox::kernel_status(&state).await.pid;
    let old_bytes = tokio::fs::read(&state.runtime_paths.active_config)
        .await
        .unwrap();
    let background_state = state.clone();
    let background = tokio::spawn(async move {
        super::super::refresh_subscriptions_in_background(&config, &background_state).await;
    });

    wait_for_slow_retry(&state).await;
    assert_eq!(calls.load(Ordering::Relaxed), FAST_REQUESTS);
    assert!(state.runtime_ready.load(Ordering::Relaxed));
    assert_eq!(state.runtime_phase(), RuntimePhase::Ready);
    assert_eq!(state.config.read().await.node_select, NodeSelect::Manual);
    assert_eq!(
        *state.node_select_preference.read().await,
        NodeSelect::Fastest(Region::Jp)
    );

    // Observe two slow retries: one failing, then one successful. Both must
    // remain Ready while fetching, and a failed fetch must not touch the PID
    // or runtime bytes.
    for target in [FAST_REQUESTS + 1, FAST_REQUESTS + 2] {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while received.recv().await.unwrap() != target {}
        })
        .await
        .expect("low-frequency recovery should still happen");
        assert_eq!(state.runtime_phase(), RuntimePhase::Ready);
        assert_eq!(
            state.config_warning.lock().await.as_deref(),
            Some(SUBS_RETRYING_SLOWLY)
        );
        assert_eq!(singbox::kernel_status(&state).await.pid, pid);
        assert_eq!(
            tokio::fs::read(&state.runtime_paths.active_config)
                .await
                .unwrap(),
            old_bytes
        );
        release.notify_one();
    }

    tokio::time::timeout(std::time::Duration::from_secs(2), background)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), FAST_REQUESTS + 2);
    assert_eq!(state.runtime_phase(), RuntimePhase::Ready);
    assert!(state.config_warning.lock().await.is_none());
    assert_eq!(
        state.config.read().await.node_select,
        NodeSelect::Fastest(Region::Jp)
    );
    assert!(state.runtime_paths.sub_nodes_snapshot.exists());
    singbox::stop_sing_internal(&state).await;
    server.abort();
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn stopping_service_interrupts_the_slow_retry_wait() {
    let (subscription, calls) = eventually_available_subscription_server(usize::MAX).await;
    let config = manual_config(subscription);
    let (state, root) = local_startup_test_state(config.clone(), "stop-slow-retry").await;
    assert!(initialize_runtime_locked(&config, &state).await);
    let background_state = state.clone();
    let background = tokio::spawn(async move {
        super::super::refresh_subscriptions_in_background(&config, &background_state).await;
    });
    wait_for_slow_retry(&state).await;
    assert_eq!(calls.load(Ordering::Relaxed), FAST_REQUESTS);

    assert!(
        crate::handlers::service::stop_service(axum::extract::State(state.clone()))
            .await
            .is_ok()
    );
    tokio::time::timeout(std::time::Duration::from_millis(200), background)
        .await
        .expect("stop must not wait for the low-frequency retry timer")
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), FAST_REQUESTS);
    assert_eq!(state.runtime_phase(), RuntimePhase::Stopped);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn changing_subscriptions_cancels_the_old_slow_retry_task() {
    let (subscription, calls) = eventually_available_subscription_server(usize::MAX).await;
    let config = manual_config(subscription);
    let (state, root) = local_startup_test_state(config.clone(), "edit-slow-retry").await;
    assert!(initialize_runtime_locked(&config, &state).await);
    let background_state = state.clone();
    let background = tokio::spawn(async move {
        super::super::refresh_subscriptions_in_background(&config, &background_state).await;
    });
    wait_for_slow_retry(&state).await;
    {
        // Model a committed subscription edit without initiating another fetch.
        let _guard = state.config_update.lock().await;
        state.config.write().await.subs.clear();
        state.next_sub_refresh();
    }
    tokio::time::timeout(std::time::Duration::from_millis(200), background)
        .await
        .expect("editing subscriptions must interrupt the long wait")
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), FAST_REQUESTS);
    assert!(state.runtime_ready.load(Ordering::Relaxed));
    singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn foreground_success_during_slow_wait_prevents_another_background_request() {
    let (subscription, calls) = eventually_available_subscription_server(FAST_REQUESTS).await;
    let config = manual_config(subscription);
    let (state, root) = local_startup_test_state(config.clone(), "foreground-slow-retry").await;
    assert!(initialize_runtime_locked(&config, &state).await);
    let background_state = state.clone();
    let background = tokio::spawn(async move {
        super::super::refresh_subscriptions_in_background(&config, &background_state).await;
    });
    wait_for_slow_retry(&state).await;

    crate::services::config::refresh_subscriptions_foreground(&state)
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), background)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), FAST_REQUESTS + 1);
    assert!(state.config_warning.lock().await.is_none());
    assert_eq!(state.runtime_phase(), RuntimePhase::Ready);
    singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn failed_foreground_refresh_does_not_reset_the_fast_retry_budget() {
    let (subscription, calls) = eventually_available_subscription_server(usize::MAX).await;
    let config = manual_config(subscription);
    let (state, root) = local_startup_test_state(config.clone(), "foreground-failed-slow").await;
    assert!(initialize_runtime_locked(&config, &state).await);
    let background_state = state.clone();
    let background = tokio::spawn(async move {
        super::super::refresh_subscriptions_in_background(&config, &background_state).await;
    });
    wait_for_slow_retry(&state).await;
    crate::services::config::refresh_subscriptions_foreground(&state)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(
        calls.load(Ordering::Relaxed),
        FAST_REQUESTS + 1,
        "a foreground failure must not restart rapid background polling"
    );

    assert!(
        crate::handlers::service::stop_service(axum::extract::State(state.clone()))
            .await
            .is_ok()
    );
    tokio::time::timeout(std::time::Duration::from_millis(200), background)
        .await
        .unwrap()
        .unwrap();
    let _ = tokio::fs::remove_dir_all(root).await;
}
