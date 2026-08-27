use super::{restart_backoff, tun_adapter_cleanup_command, TUN_ADAPTER_NAME};
use tokio::time::Duration;

#[cfg(unix)]
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

#[cfg(unix)]
#[tokio::test]
async fn failed_probe_cleanup_keeps_the_watcher_generation_retryable() {
    use std::sync::atomic::Ordering;
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
    state.sing_generation.store(4, Ordering::Relaxed);

    super::terminate_failed_start(&state, 4).await;

    assert!(state.sing_process.lock().await.is_none());
    assert_eq!(state.sing_generation.load(Ordering::Relaxed), 4);
    assert!(super::start_still_current(&state, 4));
}

#[cfg(unix)]
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
    state
        .sing_generation
        .store(7, std::sync::atomic::Ordering::Relaxed);

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

#[cfg(unix)]
#[tokio::test]
async fn watcher_returns_without_restart_when_service_should_not_run() {
    let state = crate::test_support::app_state(crate::models::Config::default());
    state
        .service_should_run
        .store(false, std::sync::atomic::Ordering::Relaxed);
    state
        .sing_generation
        .store(4, std::sync::atomic::Ordering::Relaxed);

    // 槽位为空且 generation 匹配 → 视为异常退出；但服务已被明确停止，直接退出
    tokio::time::timeout(
        Duration::from_secs(5),
        super::watch_sing_box(state.clone(), 4),
    )
    .await
    .expect("watcher should exit when service should not run");
    assert!(state.config_warning.lock().await.is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn reload_keeps_the_existing_process() {
    use crate::models::RuntimePhase;
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    let state = crate::test_support::app_state(crate::models::Config::default());
    let child = tokio::process::Command::new("sh")
        .args(["-c", "trap ':' HUP; while :; do sleep 1; done"])
        .kill_on_drop(true)
        .spawn()
        .expect("spawn reload-aware child");
    let pid = child.id().expect("child pid");
    *state.sing_process.lock().await = Some(crate::state::SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    state
        .sing_generation
        .store(7, std::sync::atomic::Ordering::Relaxed);
    state.runtime_ready.store(true, Ordering::Relaxed);
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
    assert!(state.runtime_ready.load(Ordering::Relaxed));
    assert_eq!(state.runtime_phase(), RuntimePhase::Ready);
    assert_eq!(state.sing_generation.load(Ordering::Relaxed), 8);

    super::stop_sing_internal(&state).await;
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_and_probe_refuses_stale_generation() {
    let state = crate::test_support::app_state(crate::models::Config::default());
    state
        .sing_generation
        .store(9, std::sync::atomic::Ordering::Relaxed);

    let err = super::spawn_and_probe_sing_box(&state, 3)
        .await
        .expect_err("stale generation");
    assert!(err.to_string().contains("cancelled"));
    assert!(state.sing_process.lock().await.is_none());
}

#[cfg(unix)]
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
    state
        .sing_generation
        .store(1, std::sync::atomic::Ordering::Relaxed);

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
    super::clear_kernel_give_up_warning(&state).await;
    assert!(state.config_warning.lock().await.is_none());

    *state.config_warning.lock().await = Some("所有订阅获取失败，请检查当前订阅".to_string());
    super::clear_kernel_give_up_warning(&state).await;
    assert_eq!(
        state.config_warning.lock().await.as_deref(),
        Some("所有订阅获取失败，请检查当前订阅")
    );
}

#[test]
fn tun_adapter_cleanup_targets_sing_tun_only() {
    // Assembled for Windows callers; this test never executes the command.
    let (program, args) = tun_adapter_cleanup_command();
    let joined = args.join(" ");
    assert_eq!(program, "powershell.exe");
    assert!(joined.contains(TUN_ADAPTER_NAME));
    assert!(joined.contains("Remove-NetAdapter"));
    assert!(!joined.contains("Get-NetAdapter -Name '*'"));
}

#[test]
fn windows_stop_signal_targets_process_group() {
    assert_eq!(super::WINDOWS_CREATE_NEW_PROCESS_GROUP, 0x0000_0200);
    assert_eq!(super::WINDOWS_CTRL_BREAK_EVENT, 0x0000_0001);
    assert_eq!(super::WINDOWS_JOB_KILL_ON_CLOSE, 0x0000_2000);
}

#[test]
fn sharing_violation_codes_are_detected() {
    assert!(super::is_windows_sharing_violation(
        &std::io::Error::from_raw_os_error(32)
    ));
    assert!(super::is_windows_sharing_violation(
        &std::io::Error::from_raw_os_error(33)
    ));
    assert!(!super::is_windows_sharing_violation(
        &std::io::Error::from_raw_os_error(2)
    ));
    let message =
        super::map_remove_embedded_error("sing-box.exe", std::io::Error::from_raw_os_error(32));
    assert!(message.to_string().contains("残留的 sing-box"));
}
