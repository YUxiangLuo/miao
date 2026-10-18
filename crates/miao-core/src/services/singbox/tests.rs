use super::restart_backoff;
use tokio::time::Duration;

#[test]
fn restrict_to_owner_sets_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("miao-perms-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    super::restrict_to_owner(&dir).unwrap();

    let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn restart_backoff_grows_exponentially_and_caps() {
    assert_eq!(restart_backoff(1), Duration::from_secs(1));
    assert_eq!(restart_backoff(2), Duration::from_secs(2));
    assert_eq!(restart_backoff(3), Duration::from_secs(4));
    assert_eq!(restart_backoff(5), Duration::from_secs(16));
    assert_eq!(restart_backoff(42), Duration::from_secs(16));
}

#[tokio::test]
async fn failed_probe_cleanup_keeps_the_watcher_generation_retryable() {
    use std::time::Instant;

    let state = crate::test_support::app_state(crate::models::Config::default());
    let child = tokio::process::Command::new("sleep")
        .arg("30")
        .kill_on_drop(true)
        .spawn()
        .expect("spawn dummy child");
    *state.sing_process.lock().await = Some(crate::state::SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    while state.lifecycle.snapshot().generation < 4 {
        state
            .lifecycle
            .begin(crate::state::lifecycle::KernelOperation::Start);
    }

    super::terminate_failed_start(&state, 4).await;

    assert!(state.sing_process.lock().await.is_none());
    assert_eq!(state.lifecycle.snapshot().generation, 4);
    assert!(super::start_still_current(&state, 4));
}

#[tokio::test]
async fn watcher_exits_when_its_generation_is_stale() {
    use std::time::Instant;

    let state = crate::test_support::app_state(crate::models::Config::default());
    let child = tokio::process::Command::new("sleep")
        .arg("30")
        .kill_on_drop(true)
        .spawn()
        .expect("spawn dummy child");
    *state.sing_process.lock().await = Some(crate::state::SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    while state.lifecycle.snapshot().generation < 7 {
        state
            .lifecycle
            .begin(crate::state::lifecycle::KernelOperation::Start);
    }

    tokio::time::timeout(
        Duration::from_secs(5),
        super::watch_sing_box(state.clone(), 3),
    )
    .await
    .expect("stale watcher should exit");

    // 过时的看门狗不得收割或重启仍在运行的进程
    let mut lock = state.sing_process.lock().await;
    let proc = lock.as_mut().expect("dummy child must be kept");
    assert!(proc.child.try_wait().expect("poll").is_none());
}

#[tokio::test]
async fn watcher_returns_without_restart_when_service_should_not_run() {
    let state = crate::test_support::app_state(crate::models::Config::default());
    state.lifecycle.request_running(false);
    while state.lifecycle.snapshot().generation < 4 {
        state
            .lifecycle
            .begin(crate::state::lifecycle::KernelOperation::Start);
    }

    // 槽位为空且 generation 匹配 → 视为异常退出；但服务已被明确停止，直接退出
    tokio::time::timeout(
        Duration::from_secs(5),
        super::watch_sing_box(state.clone(), 4),
    )
    .await
    .expect("watcher should exit when service should not run");
    assert!(state.config_warning.lock().await.is_none());
}

#[tokio::test]
async fn reload_keeps_the_existing_process() {
    use crate::models::RuntimePhase;
    use std::time::Instant;

    let (_root, state) = crate::test_support::isolated_stopped_state(Default::default());
    state.lifecycle.request_running(true);
    std::fs::write(&state.runtime_paths.active_config, "{}").unwrap();
    super::activation::Activation::prepare(&state)
        .await
        .unwrap();
    let child = tokio::process::Command::new("sh")
        .args(["-c", crate::test_support::FAKE_KERNEL])
        .current_dir(&state.runtime_paths.runtime_dir)
        .kill_on_drop(true)
        .spawn()
        .expect("spawn reload-aware child");
    let pid = child.id().expect("child pid");
    *state.sing_process.lock().await = Some(crate::state::SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    while state.lifecycle.snapshot().generation < 7 {
        state
            .lifecycle
            .begin(crate::state::lifecycle::KernelOperation::Start);
    }
    state.lifecycle.finish(
        state.lifecycle.snapshot().generation,
        crate::models::RuntimePhase::Ready,
    );
    // Let the shell install its trap before delivering SIGHUP.
    tokio::time::sleep(Duration::from_millis(50)).await;

    super::reload_sing_internal(&state)
        .await
        .expect("reload should keep a signal-aware child alive");

    let mut lock = state.sing_process.lock().await;
    let process = lock.as_mut().expect("child remains tracked");
    assert_eq!(process.child.id(), Some(pid));
    assert!(process.child.try_wait().expect("poll child").is_none());
    drop(lock);
    assert!(state.lifecycle.snapshot().ready);
    assert_eq!(state.lifecycle.snapshot().phase, RuntimePhase::Ready);
    assert_eq!(state.lifecycle.snapshot().generation, 8);

    super::stop_sing_internal(&state).await;
}

#[tokio::test]
async fn spawn_and_probe_refuses_stale_generation() {
    let state = crate::test_support::app_state(crate::models::Config::default());
    while state.lifecycle.snapshot().generation < 9 {
        state
            .lifecycle
            .begin(crate::state::lifecycle::KernelOperation::Start);
    }

    let err = super::spawn_and_probe_sing_box(&state, 3)
        .await
        .expect_err("stale generation");
    assert!(err.to_string().contains("cancelled"));
    assert!(state.sing_process.lock().await.is_none());
}

#[tokio::test]
async fn spawn_and_probe_refuses_to_overwrite_a_live_child() {
    use std::time::Instant;

    let state = crate::test_support::app_state(crate::models::Config::default());
    let child = tokio::process::Command::new("sleep")
        .arg("30")
        .kill_on_drop(true)
        .spawn()
        .expect("spawn dummy child");
    *state.sing_process.lock().await = Some(crate::state::SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    while state.lifecycle.snapshot().generation < 1 {
        state
            .lifecycle
            .begin(crate::state::lifecycle::KernelOperation::Start);
    }

    let err = super::spawn_and_probe_sing_box(&state, 1)
        .await
        .expect_err("live child");
    assert!(matches!(err, crate::error::AppError::AlreadyRunning));

    let mut lock = state.sing_process.lock().await;
    let proc = lock.as_mut().expect("dummy child must be kept");
    assert!(proc.child.try_wait().expect("poll").is_none());
}

#[tokio::test]
async fn successful_start_clears_only_the_give_up_warning() {
    let state = crate::test_support::app_state(crate::models::Config::default());
    *state.config_warning.lock().await = Some(super::KERNEL_GIVE_UP_WARNING.to_string());
    super::clear_kernel_give_up_warning(&state, state.lifecycle.snapshot().generation).await;
    assert!(state.config_warning.lock().await.is_none());

    *state.config_warning.lock().await = Some("所有订阅获取失败，请检查当前订阅".to_string());
    super::clear_kernel_give_up_warning(&state, state.lifecycle.snapshot().generation).await;
    assert_eq!(
        state.config_warning.lock().await.as_deref(),
        Some("所有订阅获取失败，请检查当前订阅")
    );
}
