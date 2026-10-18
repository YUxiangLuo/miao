use super::*;
use axum::{routing::get, Router};
use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicBool, AtomicUsize},
};

mod upgrade_health;

const OLD: &str = "proxies:\n- {name: remote, type: trojan, server: 192.0.2.11, port: 443, password: old-password}\n";
const NEW: &str = "proxies:\n- {name: remote, type: trojan, server: 192.0.2.22, port: 443, password: new-password}\n";

fn config(url: String) -> Config {
    Config {
        subs: vec![url],
        nodes: vec![r#"{"type":"trojan","tag":"manual","server":"192.0.2.1","server_port":443,"password":"test"}"#.to_string()],
        ..Default::default()
    }
}

fn fixture(config: Config, fail_first_run: bool) -> (tempfile::TempDir, Arc<AppState>) {
    let (root, state) = crate::test_support::isolated_stopped_state(config);
    state.lifecycle.request_running(true);
    state
        .lifecycle
        .finish(state.lifecycle.snapshot().generation, RuntimePhase::Failed);
    let mut script = crate::test_support::FAKE_KERNEL.to_string();
    if fail_first_run {
        script = script.replace(
            "miao_ack() {",
            "if [ ! -e failed-once ]; then touch failed-once; exit 1; fi\nmiao_ack() {",
        );
    }
    let path = state.runtime_paths.runtime_dir.join("sing-box");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    (root, state)
}

struct SubscriptionServer {
    url: String,
    calls: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for SubscriptionServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl SubscriptionServer {
    async fn start() -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let (count, first_in, first_out) = (calls.clone(), entered.clone(), release.clone());
        let app = Router::new().route(
            "/sub",
            get(move || {
                let (count, first_in, first_out) =
                    (count.clone(), first_in.clone(), first_out.clone());
                async move {
                    if count.fetch_add(1, Ordering::Relaxed) == 0 {
                        first_in.notify_one();
                        first_out.notified().await;
                        OLD
                    } else {
                        NEW
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/sub", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            url,
            calls,
            entered,
            release,
            task,
        }
    }
}

async fn password(state: &AppState) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(&state.runtime_paths.active_config)
            .await
            .unwrap(),
    )
    .unwrap();
    json["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| ["192.0.2.11", "192.0.2.22"].contains(&node["server"].as_str().unwrap_or("")))
        .and_then(|node| node["password"].as_str())
        .map(str::to_string)
}

#[tokio::test]
async fn duplicate_background_recovery_cannot_overwrite_newer_credentials() {
    let server = SubscriptionServer::start().await;
    let cfg = config(server.url.clone());
    let (_root, state) = fixture(cfg.clone(), false);
    assert!(initialize_runtime_locked(&cfg, &state).await);
    let old_state = state.clone();
    let older = tokio::spawn(async move { retry_recovered_subscriptions(&old_state).await });
    tokio::time::timeout(Duration::from_secs(1), server.entered.notified())
        .await
        .unwrap();
    let new_state = state.clone();
    let mut newer = tokio::spawn(async move { retry_recovered_subscriptions(&new_state).await });
    // Before the fix this second owner commits NEW while OLD is still pending.
    // A correct coordinator may instead coalesce the second entrant entirely.
    let finished_early = tokio::time::timeout(Duration::from_millis(750), &mut newer).await;
    let before = password(&state).await;
    server.release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), older)
        .await
        .unwrap()
        .unwrap();
    match finished_early {
        Ok(result) => result.unwrap(),
        Err(_) => tokio::time::timeout(Duration::from_secs(2), newer)
            .await
            .unwrap()
            .unwrap(),
    }
    let after = password(&state).await;
    let calls = server.calls.load(Ordering::Relaxed);
    stop_sing_internal(&state).await;
    if before.as_deref() == Some("new-password") {
        assert_eq!(
            after, before,
            "old recovery response replaced newer committed credentials"
        );
    }
    assert_eq!(
        calls, 1,
        "one recovery generation must have one active background owner"
    );
    assert_eq!(after.as_deref(), Some("old-password"));
}

#[tokio::test]
async fn startup_and_manual_start_share_the_handoff_after_local_recovery() {
    let calls = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let (count, arrived, resume) = (calls.clone(), entered.clone(), release.clone());
    let app = Router::new().route(
        "/sub",
        get(move || {
            let (count, arrived, resume) = (count.clone(), arrived.clone(), resume.clone());
            async move {
                match count.fetch_add(1, Ordering::Relaxed) + 1 {
                    1..=3 => (axum::http::StatusCode::SERVICE_UNAVAILABLE, "offline"),
                    4 => {
                        arrived.notify_one();
                        resume.notified().await;
                        (axum::http::StatusCode::OK, OLD)
                    }
                    _ => (axum::http::StatusCode::OK, NEW),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = config(format!("http://{}/sub", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_root, state) = fixture(cfg, false);
    let a_state = state.clone();
    let startup = tokio::spawn(async move { retry_failed_startup(&a_state).await });
    let b_state = state.clone();
    // Same sequence as start_service after extraction: recover, then hand off.
    let manual = tokio::spawn(async move {
        if recover_data_plane_once(&b_state).await == RecoveryOutcome::RetrySubscriptions {
            retry_recovered_subscriptions(&b_state).await;
        }
    });
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(75)).await;
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(3), startup)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), manual)
        .await
        .unwrap()
        .unwrap();
    let actual = password(&state).await;
    stop_sing_internal(&state).await;
    server.abort();
    assert_eq!(
        calls.load(Ordering::Relaxed),
        4,
        "three failed startup requests and ONE shared recovery fetch"
    );
    assert_eq!(actual.as_deref(), Some("old-password"));
}

#[tokio::test]
async fn manual_recovery_joins_an_initial_recovery_attempt() {
    recovery_attempts_are_serialized(false).await;
}

#[tokio::test]
async fn manual_recovery_joins_a_background_fetch_after_kernel_failure() {
    recovery_attempts_are_serialized(true).await;
}

async fn recovery_attempts_are_serialized(background: bool) {
    let server = SubscriptionServer::start().await;
    let cfg = config(server.url.clone());
    let (_root, state) = fixture(cfg.clone(), false);
    if background {
        assert!(initialize_runtime_locked(&cfg, &state).await);
    }
    let a_state = state.clone();
    let first = tokio::spawn(async move {
        if background {
            retry_recovered_subscriptions(&a_state).await;
        } else {
            assert_eq!(
                recover_data_plane_once(&a_state).await,
                RecoveryOutcome::Complete
            );
        }
    });
    tokio::time::timeout(Duration::from_secs(1), server.entered.notified())
        .await
        .unwrap();
    if background {
        stop_sing_internal(&state).await;
    } // keep should_run; simulate a lost kernel
    let b_state = state.clone();
    let second = tokio::spawn(async move { recover_data_plane_once(&b_state).await });
    tokio::time::sleep(Duration::from_millis(75)).await;
    server.release.notify_one();
    tokio::time::timeout(Duration::from_secs(3), first)
        .await
        .unwrap()
        .unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(3), second)
        .await
        .unwrap()
        .unwrap();
    let calls = server.calls.load(Ordering::Relaxed);
    stop_sing_internal(&state).await;
    assert_eq!(
        calls, 1,
        "manual/startup recovery must reuse an in-flight attempt, not race its commit"
    );
    assert_eq!(outcome, RecoveryOutcome::Complete);
}

#[tokio::test]
async fn queued_owner_handles_changed_subscriptions_after_retiring_the_old_owner() {
    let server = SubscriptionServer::start().await;
    let cfg = config(server.url.clone());
    let (_root, state) = fixture(cfg.clone(), false);
    assert!(initialize_runtime_locked(&cfg, &state).await);
    let a_state = state.clone();
    let first = tokio::spawn(async move { retry_recovered_subscriptions(&a_state).await });
    tokio::time::timeout(Duration::from_secs(1), server.entered.notified())
        .await
        .unwrap();
    {
        let _config = state.config_update.lock().await;
        state.config.write().await.subs = vec![format!("{}?new", server.url)];
        state.next_sub_refresh();
    }
    tokio::time::timeout(
        Duration::from_secs(2),
        retry_recovered_subscriptions(&state),
    )
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(1), first)
        .await
        .unwrap()
        .unwrap();
    server.release.notify_one();
    assert_eq!(password(&state).await.as_deref(), Some("new-password"));
    // A late handoff for this completed generation must not start a third fetch.
    tokio::time::timeout(
        Duration::from_secs(1),
        retry_recovered_subscriptions(&state),
    )
    .await
    .unwrap();
    stop_sing_internal(&state).await;
    assert_eq!(server.calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn a_new_data_plane_repair_does_not_inherit_an_old_completion() {
    let offline = Arc::new(AtomicBool::new(false));
    let serving = offline.clone();
    let app = Router::new().route(
        "/sub",
        get(move || {
            let offline = serving.clone();
            async move {
                if offline.load(Ordering::Relaxed) {
                    (axum::http::StatusCode::SERVICE_UNAVAILABLE, "offline")
                } else {
                    (axum::http::StatusCode::OK, OLD)
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = config(format!("http://{}/sub", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_root, state) = fixture(cfg, false);
    assert_eq!(
        recover_data_plane_once(&state).await,
        RecoveryOutcome::Complete
    );
    stop_sing_internal(&state).await;
    offline.store(true, Ordering::Relaxed);
    let repaired = recover_data_plane_once(&state).await;
    let still_needs_subscriptions = recover_data_plane_once(&state).await;
    stop_sing_internal(&state).await;
    server.abort();
    assert_eq!(repaired, RecoveryOutcome::RetrySubscriptions);
    assert_eq!(
        still_needs_subscriptions,
        RecoveryOutcome::RetrySubscriptions
    );
}

#[tokio::test]
async fn cancelled_owner_releases_recovery_for_a_later_entrant() {
    let server = SubscriptionServer::start().await;
    let cfg = config(server.url.clone());
    let (_root, state) = fixture(cfg.clone(), false);
    assert!(initialize_runtime_locked(&cfg, &state).await);
    let a_state = state.clone();
    let first = tokio::spawn(async move { retry_recovered_subscriptions(&a_state).await });
    tokio::time::timeout(Duration::from_secs(1), server.entered.notified())
        .await
        .unwrap();
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    server.release.notify_one();
    tokio::time::timeout(
        Duration::from_secs(2),
        retry_recovered_subscriptions(&state),
    )
    .await
    .unwrap();
    let actual = password(&state).await;
    stop_sing_internal(&state).await;
    assert_eq!(actual.as_deref(), Some("new-password"));
    assert_eq!(server.calls.load(Ordering::Relaxed), 2);
}
