use super::*;
use crate::{models::Config, test_support::isolated_stopped_state};
use axum::{routing::get, Router};
use std::os::unix::fs::PermissionsExt;

async fn fixture() -> (
    tempfile::TempDir,
    Arc<AppState>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
    tokio::task::JoinHandle<()>,
) {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let (requested, resume) = (entered.clone(), release.clone());
    let app = Router::new().route("/sub", get(move || {
        let (requested, resume) = (requested.clone(), resume.clone());
        async move {
            requested.notify_one();
            resume.notified().await;
            "proxies:\n- {name: new-node, type: trojan, server: 192.0.2.2, port: 443, password: test}\n"
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/sub", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (root, state) = isolated_stopped_state(Config {
        subs: vec![url],
        ..Default::default()
    });
    let kernel = state.runtime_paths.runtime_dir.join("sing-box");
    std::fs::write(&kernel, "#!/bin/sh\n[ \"$1\" = check ]\n").unwrap();
    std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();
    (root, state, entered, release, server)
}

#[tokio::test]
async fn queued_refresh_cannot_commit_after_shutdown_returns() {
    let (_root, state, _entered, release, server) = fixture().await;
    let blocked = state.config_update.lock().await;
    let scheduled_state = state.clone();
    let mut scheduled = Box::pin(async move { fire(&scheduled_state).await });
    assert!(futures::poll!(&mut scheduled).is_pending());
    let mut closing = Box::pin(crate::runtime::shutdown_runtime(&state));
    assert!(futures::poll!(&mut closing).is_pending());
    assert!(state.lifecycle.is_shutting_down());
    drop(blocked);
    let task = tokio::spawn(scheduled);
    // Either cancellation or draining is valid; neither may commit after close.
    let closed_early = tokio::time::timeout(Duration::from_millis(100), &mut closing)
        .await
        .is_ok();
    let existed_on_shutdown = state.runtime_paths.active_config.exists();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    if !closed_early {
        tokio::time::timeout(Duration::from_secs(2), closing)
            .await
            .unwrap();
    }
    let exists_after_shutdown = state.runtime_paths.active_config.exists();
    server.abort();
    assert!(
        !closed_early || existed_on_shutdown == exists_after_shutdown,
        "scheduler must be cancelled or drained before shutdown returns"
    );
    assert!(matches!(fire(&state).await, FireOutcome::Skipped));
}

#[tokio::test]
async fn upgrade_drains_a_scheduler_fetch_outside_the_config_lock() {
    let (_root, state, entered, release, server) = fixture().await;
    let scheduled_state = state.clone();
    let task = tokio::spawn(async move { fire(&scheduled_state).await });
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .unwrap();
    let upgrading = state.clone();
    let mut upgrade = tokio::spawn(async move { upgrading.quiesce_for_upgrade().await });
    let finished_early = tokio::time::timeout(Duration::from_millis(50), &mut upgrade).await;
    release.notify_one();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap(),
        FireOutcome::Succeeded
    ));
    let waited = finished_early.is_err();
    let quiescence = match finished_early {
        Ok(result) => result.unwrap().unwrap(),
        Err(_) => tokio::time::timeout(Duration::from_secs(2), upgrade)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
    };
    assert!(state.runtime_paths.active_config.exists());
    drop(quiescence);
    server.abort();
    assert!(
        waited,
        "upgrade must wait for accepted background network/commit work"
    );
}
