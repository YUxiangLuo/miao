//! Hermetic controller races: only an isolated shell child, no TUN or Clash.
use super::*;
use crate::models::{Config, RouteMode, StableConfig};
use crate::paths::RuntimePaths;
use std::os::unix::fs::PermissionsExt;

async fn fixture() -> (tempfile::TempDir, Arc<AppState>) {
    let root = tempfile::tempdir().unwrap();
    let config_path = root.path().join("config.yaml");
    let runtime = root.path().join("runtime");
    tokio::fs::create_dir_all(&runtime).await.unwrap();
    let binary = runtime.join("sing-box");
    tokio::fs::write(&binary, b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\ntrap ':' HUP\nwhile :; do sleep 0.02; done\n").await.unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = Config {
        nodes: vec![serde_json::json!({"type":"trojan", "tag":"manual", "server":"192.0.2.1", "server_port":443, "password":"fixture"}).to_string()],
        ..Default::default()
    };
    let state = Arc::new(
        AppState::with_config_layers(
            StableConfig::from(&config),
            config.clone(),
            config_path.clone(),
            root.path().join("volatile.yaml"),
            RuntimePaths::new(runtime, &config_path),
        )
        .unwrap(),
    );
    let prepared = crate::services::config::gen_config_from_nodes(&config, &state, Vec::new())
        .await
        .unwrap();
    crate::services::config::install_prepared_runtime(&state, &prepared)
        .await
        .unwrap();
    (root, state)
}

async fn wait_for_phase(state: &AppState, phase: RuntimePhase) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if state.lifecycle.snapshot().phase == phase {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn stop_during_start_cannot_be_overwritten_by_the_cancelled_probe() {
    let (_root, state) = fixture().await;
    let other = state.clone();
    let start = tokio::spawn(async move { start_sing_internal(&other).await });
    wait_for_phase(&state, RuntimePhase::Starting).await;
    stop_sing_internal(&state).await;
    assert!(start.await.unwrap().is_err());
    let status = kernel_status(&state).await;
    assert!(!status.running);
    assert!(!status.ready);
    assert_eq!(status.phase, RuntimePhase::Stopped);
}

#[tokio::test]
async fn stop_during_reload_cannot_be_overwritten_by_the_cancelled_probe() {
    let (_root, state) = fixture().await;
    start_sing_internal(&state).await.unwrap();
    let other = state.clone();
    let reload = tokio::spawn(async move { reload_sing_internal(&other).await });
    wait_for_phase(&state, RuntimePhase::Reloading).await;
    stop_sing_internal(&state).await;
    assert!(reload.await.unwrap().is_err());
    assert_eq!(state.lifecycle.snapshot().phase, RuntimePhase::Stopped);
    assert!(!state.lifecycle.snapshot().ready);
    assert!(!kernel_status(&state).await.running);
}

#[tokio::test]
async fn config_change_during_watchdog_backoff_retires_the_old_recovery() {
    let (_root, state) = fixture().await;
    start_sing_internal(&state).await.unwrap();
    let old_generation = state.lifecycle.snapshot().generation;
    {
        let mut slot = state.sing_process.lock().await;
        let process = slot.as_mut().unwrap();
        process.child.start_kill().unwrap();
        process.child.wait().await.unwrap();
    }
    wait_for_phase(&state, RuntimePhase::Starting).await;
    crate::services::config::apply_route_mode(&state, RouteMode::Global)
        .await
        .unwrap();
    let healthy = kernel_status(&state).await;
    assert!(healthy.ready);
    assert!(state.lifecycle.snapshot().generation > old_generation);
    // The previous watcher wakes after 1s, but cannot touch the replacement.
    sleep(Duration::from_millis(1200)).await;
    assert_eq!(kernel_status(&state).await.pid, healthy.pid);
    assert!(state.lifecycle.snapshot().ready);
    assert_eq!(state.config.read().await.route_mode, RouteMode::Global);
    stop_sing_internal(&state).await;
}

#[tokio::test]
async fn stale_warning_cleanup_and_ready_publication_leave_the_new_owner_untouched() {
    let (_root, state) = fixture().await;
    let old = state.lifecycle.snapshot().generation;
    start_sing_internal(&state).await.unwrap();
    *state.config_warning.lock().await = Some(KERNEL_GIVE_UP_WARNING.to_string());
    clear_kernel_give_up_warning(&state, old).await;
    let before = state.lifecycle.snapshot();
    assert!(publish_kernel_ready(&state, old).await.is_err());
    assert_eq!(state.lifecycle.snapshot(), before);
    assert_eq!(
        state.config_warning.lock().await.as_deref(),
        Some(KERNEL_GIVE_UP_WARNING)
    );
    stop_sing_internal(&state).await;
}

#[tokio::test]
async fn rollback_of_a_live_but_unhealthy_child_requires_a_new_probe() {
    let (_root, state) = fixture().await;
    start_sing_internal(&state).await.unwrap();
    let old = state.lifecycle.snapshot().generation;
    let pid = kernel_status(&state).await.pid;
    state.lifecycle.finish(old, RuntimePhase::Failed);
    let _config_update = state.config_update.lock().await;
    let previous = state.config.read().await.clone();
    let invalid = Config {
        nodes: vec!["not-json".to_string()],
        ..previous.clone()
    };
    assert!(
        crate::services::config::apply_config_change(&state, &previous, &invalid)
            .await
            .is_err()
    );
    assert!(
        state.lifecycle.snapshot().generation > old,
        "disk rollback alone must not grant readiness"
    );
    let status = kernel_status(&state).await;
    assert_eq!(status.pid, pid);
    assert!(status.ready);
    assert_eq!(status.phase, RuntimePhase::Ready);
    stop_sing_internal(&state).await;
}

#[tokio::test]
async fn a_late_start_request_cannot_revive_a_shutting_down_server() {
    let (_root, state) = fixture().await;
    start_sing_internal(&state).await.unwrap();
    state.lifecycle.shutdown();
    stop_sing_internal(&state).await;
    state.lifecycle.request_running(true);
    assert!(start_sing_internal(&state).await.is_err());
    assert_eq!(kernel_status(&state).await.phase, RuntimePhase::Stopped);
    assert!(!state.lifecycle.snapshot().should_run);
    assert!(state.sing_process.lock().await.is_none());
}

#[tokio::test]
async fn exited_child_cannot_be_published_as_ready() {
    let (_root, state) = fixture().await;
    start_sing_internal(&state).await.unwrap();
    let generation = state.lifecycle.snapshot().generation;
    {
        let mut slot = state.sing_process.lock().await;
        let child = &mut slot.as_mut().unwrap().child;
        child.start_kill().unwrap();
        child.wait().await.unwrap();
    }
    assert!(publish_kernel_ready(&state, generation).await.is_err());
    let status = kernel_status(&state).await;
    assert!(!status.running);
    assert!(!status.ready);
    assert_eq!(status.phase, RuntimePhase::Failed);
    stop_sing_internal(&state).await;
}
