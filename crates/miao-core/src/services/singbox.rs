use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::RuntimePhase;
use crate::state::lifecycle::KernelOperation;
use crate::state::{AppState, SingBoxProcess};

mod activation;
mod embedded;
mod readiness;
pub use embedded::extract_sing_box_to;

pub const CLASH_API_HOST: &str = "127.0.0.1:6262";
pub const CLASH_API_BASE: &str = "http://127.0.0.1:6262";
pub const CLASH_TRAFFIC_WS: &str = "ws://127.0.0.1:6262/traffic";

pub fn clash_api_url(path: &str) -> String {
    format!("{CLASH_API_BASE}{path}")
}

pub fn get_sing_box_home() -> PathBuf {
    PathBuf::from("/tmp/miao-sing-box")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KernelStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
    pub ready: bool,
    pub phase: RuntimePhase,
}

pub async fn kernel_status(state: &AppState) -> KernelStatus {
    let mut lock = state.sing_process.lock().await;
    let mut status = KernelStatus::default();
    if let Some(process) = &mut *lock {
        match process.child.try_wait() {
            Ok(None) => {
                status.running = true;
                status.pid = process.child.id();
                status.uptime_secs = Some(process.started_at.elapsed().as_secs());
            }
            Ok(Some(_)) => {
                *lock = None;
                let runtime = state.lifecycle.snapshot();
                state.lifecycle.finish(
                    runtime.generation,
                    if runtime.should_run {
                        RuntimePhase::Failed
                    } else {
                        RuntimePhase::Stopped
                    },
                );
            }
            Err(_) => {
                let runtime = state.lifecycle.snapshot();
                state
                    .lifecycle
                    .finish(runtime.generation, RuntimePhase::Failed);
            }
        }
    }
    // Snapshot health and process presence under the same process-slot lock.
    let runtime = state.lifecycle.snapshot();
    status.ready = status.running && runtime.ready;
    status.phase = runtime.phase;
    status
}

pub async fn is_sing_box_running(state: &AppState) -> bool {
    kernel_status(state).await.running
}

fn sing_box_file_name() -> &'static str {
    "sing-box"
}

/// 在停止运行中的实例前验证 sing-box 配置，避免不必要的服务中断。
pub async fn validate_sing_box_config(
    state: &AppState,
    config_path: &std::path::Path,
) -> AppResult<()> {
    let sing_box_home = &state.runtime_paths.runtime_dir;
    let sing_box_path = sing_box_home.join(sing_box_file_name());

    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(&sing_box_path)
            .kill_on_drop(true)
            .current_dir(sing_box_home)
            .arg("check")
            .arg("-c")
            .arg(config_path)
            .output(),
    )
    .await
    .map_err(|_| AppError::message("sing-box config check timed out after 10 seconds"))?
    .map_err(|e| AppError::context("Failed to run sing-box config check", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::message(format!(
            "Config validation failed: {}",
            stderr.trim()
        )));
    }

    Ok(())
}

pub async fn start_sing_internal(state: &Arc<AppState>) -> AppResult<()> {
    let generation = {
        let mut lock = state.sing_process.lock().await;
        if let Some(ref mut proc) = *lock {
            if proc
                .child
                .try_wait()
                .map_err(|e| {
                    AppError::context("Failed to check whether sing-box is already running", e)
                })?
                .is_none()
            {
                return Err(AppError::AlreadyRunning);
            }
        }
        if !state.lifecycle.snapshot().should_run {
            return Err(AppError::message("sing-box start was cancelled"));
        }
        state.lifecycle.begin(KernelOperation::Start)
    };

    // Capture ownership before dependency I/O too, so Stop cancels this work.

    if let Err(err) = crate::services::openwrt::check_and_install_openwrt_dependencies().await {
        error!(error = %err, "Failed to check or install OpenWrt dependencies");
    }
    if let Err(err) = spawn_and_probe_sing_box(state, generation).await {
        state.lifecycle.finish(generation, RuntimePhase::Failed);
        return Err(err);
    }
    publish_kernel_ready(state, generation).await?;
    spawn_crash_watcher(state.clone(), generation);
    clear_kernel_give_up_warning(state, generation).await;
    Ok(())
}

/// Reload the active config in place on Unix. sing-box officially wires
/// SIGHUP to its reload path; miao still verifies that the same process stays
/// alive and the data plane becomes healthy before publishing `ready` again.
pub async fn reload_sing_internal(state: &Arc<AppState>) -> AppResult<()> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    let (pid, generation, activation, signal) = {
        let mut lock = state.sing_process.lock().await;
        if !state.lifecycle.snapshot().should_run {
            return Err(AppError::message("sing-box reload was cancelled"));
        }
        let generation = state.lifecycle.begin(KernelOperation::Reload);
        let Some(process) = lock.as_mut() else {
            state.lifecycle.finish(generation, RuntimePhase::Failed);
            return Err(AppError::message("sing-box is not running"));
        };
        let exit_status = match process.child.try_wait() {
            Ok(status) => status,
            Err(err) => {
                state.lifecycle.finish(generation, RuntimePhase::Failed);
                return Err(AppError::context(
                    "Failed to check sing-box before reload",
                    err,
                ));
            }
        };
        if let Some(status) = exit_status {
            *lock = None;
            state.lifecycle.finish(generation, RuntimePhase::Failed);
            return Err(AppError::message(format!(
                "sing-box exited before reload with code {}",
                status.code().unwrap_or(-1)
            )));
        }
        let Some(pid) = process.child.id() else {
            state.lifecycle.finish(generation, RuntimePhase::Failed);
            return Err(AppError::message("sing-box process ID is unavailable"));
        };
        let activation = match activation::Activation::prepare(state).await {
            Ok(activation) => activation,
            Err(err) => {
                state.lifecycle.finish(generation, RuntimePhase::Failed);
                return Err(err);
            }
        };
        // Signal while owning the slot; Stop cannot replace/reap this PID
        // between inspection and SIGHUP delivery.
        (
            pid,
            generation,
            activation,
            kill(Pid::from_raw(pid as i32), Signal::SIGHUP),
        )
    };

    if let Err(err) = signal {
        state.lifecycle.finish(generation, RuntimePhase::Failed);
        // Do not infer health merely from a live process after a signal error.
        // The owning configuration transaction can restore and probe it.
        return Err(AppError::message(format!(
            "Failed to signal sing-box reload: {err}"
        )));
    }

    if let Err(err) = wait_for_sing_box_reload_ready(state, generation, pid, &activation).await {
        state.lifecycle.finish(generation, RuntimePhase::Failed);
        return Err(err);
    }

    publish_kernel_ready(state, generation).await?;
    spawn_crash_watcher(state.clone(), generation);
    clear_kernel_give_up_warning(state, generation).await;
    info!(pid, "sing-box configuration reloaded in place");
    Ok(())
}

async fn wait_for_sing_box_reload_ready(
    state: &Arc<AppState>,
    expected_generation: u64,
    expected_pid: u32,
    activation: &activation::Activation,
) -> AppResult<()> {
    readiness::wait_for_ready(
        state,
        expected_generation,
        readiness::Operation::Reload { pid: expected_pid },
        activation,
    )
    .await
}

/// Final publication shares the process lock with Stop and status polling.
/// A successful HTTP probe alone cannot resurrect a reaped/replaced child.
async fn publish_kernel_ready(state: &AppState, generation: u64) -> AppResult<()> {
    let mut slot = state.sing_process.lock().await;
    if !start_still_current(state, generation) {
        return Err(AppError::message("sing-box operation was cancelled"));
    }
    let alive = match slot.as_mut() {
        Some(process) => process
            .child
            .try_wait()
            .is_ok_and(|status| status.is_none()),
        None => false,
    };
    if !alive {
        state.lifecycle.finish(generation, RuntimePhase::Failed);
        return Err(AppError::message(
            "sing-box exited before readiness publication",
        ));
    }
    if !state.lifecycle.finish(generation, RuntimePhase::Ready) {
        return Err(AppError::message("sing-box operation was cancelled"));
    }
    // A repaired data plane proves this boot healthy even when subscription
    // recovery will keep retrying forever. Covers startup, manual recovery,
    // rollback and watchdog starts at the same authoritative Ready boundary.
    crate::services::version::mark_upgrade_healthy();
    Ok(())
}

/// Spawn sing-box from the current config.json. `expected_generation` must
/// still be current; a live child in the slot is never overwritten.
async fn spawn_and_probe_sing_box(
    state: &Arc<AppState>,
    expected_generation: u64,
) -> AppResult<()> {
    let mut lock = state.sing_process.lock().await;
    if !start_still_current(state, expected_generation) {
        return Err(AppError::message("sing-box start was cancelled"));
    }
    if let Some(ref mut proc) = *lock {
        if proc
            .child
            .try_wait()
            .map_err(|e| {
                AppError::context("Failed to check whether sing-box is already running", e)
            })?
            .is_none()
        {
            return Err(AppError::AlreadyRunning);
        }
    }

    let sing_box_home = &state.runtime_paths.runtime_dir;
    let sing_box_path = sing_box_home.join(sing_box_file_name());
    let config_path = &state.runtime_paths.active_config;

    info!(binary = ?sing_box_path, config = ?config_path, "Starting sing-box");

    let activation = activation::Activation::prepare(state).await?;
    let mut command = tokio::process::Command::new(&sing_box_path);
    command
        .env(activation::DIRECTORY_ENV, sing_box_home)
        .current_dir(sing_box_home)
        .arg("run")
        .arg("-c")
        .arg(config_path);

    // If start is cancelled between spawn and store, Drop must not leak the kernel.
    command.kill_on_drop(true);

    if let Some(log_path) = crate::paths::active_log_path() {
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
        {
            if let Ok(stderr) = file.try_clone() {
                command.stdout(file).stderr(stderr);
            } else {
                command
                    .stdout(std::process::Stdio::inherit())
                    .stderr(std::process::Stdio::inherit());
            }
        } else {
            command
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit());
        }
    } else {
        command
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
    }

    let child = command
        .spawn()
        .map_err(|e| AppError::context("Failed to spawn sing-box process", e))?;

    let pid = child.id();
    info!(pid = pid, "sing-box process spawned");

    // Own the child before the first await so cancelling init cannot drop it
    // untracked; stop_sing_internal then finds it.
    *lock = Some(SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    drop(lock);

    readiness::wait_for_ready(
        state,
        expected_generation,
        readiness::Operation::Start,
        &activation,
    )
    .await
}

async fn terminate_failed_start(state: &Arc<AppState>, expected_generation: u64) {
    let mut lock = state.sing_process.lock().await;
    if state.lifecycle.snapshot().generation != expected_generation {
        return;
    }
    if let Some(proc) = lock.as_mut() {
        if proc.child.try_wait().ok().flatten().is_none() {
            request_graceful_exit(&mut proc.child).await;
        }
    }
    *lock = None;
    // Do not retire this generation here. During initial startup no watcher
    // exists yet; during crash recovery the existing watcher must keep the
    // same generation so it can consume the remaining retry budget.
}

pub async fn stop_sing_internal(state: &Arc<AppState>) {
    let mut lock = state.sing_process.lock().await;
    // Retire old probes/watchers BEFORE awaiting graceful shutdown.
    let generation = state.lifecycle.begin(KernelOperation::Stop);
    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait().ok().flatten().is_none() {
            request_graceful_exit(&mut proc.child).await;
        }
    }
    *lock = None;
    state.lifecycle.finish(generation, RuntimePhase::Stopped);
}

mod watcher;

#[cfg(test)]
use watcher::watch_sing_box;
use watcher::{clear_kernel_give_up_warning, spawn_crash_watcher, start_still_current};
#[cfg(test)]
use watcher::{restart_backoff, KERNEL_GIVE_UP_WARNING};

/// unix：运行时目录设为 0700（内含订阅凭证，仅属主可入）。
fn restrict_to_owner(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

fn set_executable(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

async fn request_graceful_exit(child: &mut tokio::process::Child) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    if let Some(pid) = child.id() {
        let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
        match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
            Ok(Ok(_)) => {}
            _ => {
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod lifecycle_tests;
