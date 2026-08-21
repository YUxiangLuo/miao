use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::RuntimePhase;
use crate::state::{AppState, SingBoxProcess};

#[cfg(all(windows, target_arch = "x86_64"))]
const SING_BOX_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/sing-box-windows-amd64.exe"
));

#[cfg(all(windows, target_arch = "aarch64"))]
compile_error!("Windows arm64 is not supported yet");

#[cfg(all(not(windows), target_arch = "x86_64"))]
const SING_BOX_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/sing-box-amd64"
));

#[cfg(all(not(windows), target_arch = "aarch64"))]
const SING_BOX_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/sing-box-arm64"
));

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("Unsupported architecture: only x86_64 and aarch64 are supported. Please add support for your target architecture in embedded/ directory.");

const IP_RULE_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/geoip-cn.srs"
));
const SITE_RULE_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/geosite-geolocation-cn.srs"
));

pub const CLASH_API_HOST: &str = "127.0.0.1:6262";
pub const CLASH_API_BASE: &str = "http://127.0.0.1:6262";
pub const CLASH_TRAFFIC_WS: &str = "ws://127.0.0.1:6262/traffic";

pub fn clash_api_url(path: &str) -> String {
    format!("{CLASH_API_BASE}{path}")
}

pub fn get_sing_box_home() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::temp_dir().join("miao-sing-box")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/tmp/miao-sing-box")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KernelStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
}

pub async fn kernel_status(state: &AppState) -> KernelStatus {
    let mut lock = state.sing_process.lock().await;
    let Some(process) = &mut *lock else {
        return KernelStatus::default();
    };
    match process.child.try_wait() {
        Ok(None) => KernelStatus {
            running: true,
            pid: process.child.id(),
            uptime_secs: Some(process.started_at.elapsed().as_secs()),
        },
        Ok(Some(_)) => {
            *lock = None;
            state.runtime_ready.store(false, Ordering::Relaxed);
            state.set_runtime_phase(if state.service_should_run.load(Ordering::Relaxed) {
                RuntimePhase::Failed
            } else {
                RuntimePhase::Stopped
            });
            KernelStatus::default()
        }
        Err(_) => KernelStatus::default(),
    }
}

pub async fn is_sing_box_running(state: &AppState) -> bool {
    kernel_status(state).await.running
}

fn sing_box_file_name() -> &'static str {
    #[cfg(windows)]
    {
        "sing-box.exe"
    }
    #[cfg(not(windows))]
    {
        "sing-box"
    }
}

pub fn extract_sing_box_to(sing_box_home: &std::path::Path) -> AppResult<PathBuf> {
    if !sing_box_home.exists() {
        fs::create_dir_all(sing_box_home)
            .map_err(|e| AppError::context("Failed to create sing-box home directory", e))?;
    }
    // 运行时目录含订阅凭证（config.json / sub-nodes.json / cache.db）：仅属主可进，
    // 避免同机其他用户读取。每次启动都执行，顺带修正旧版本留下的宽松权限；
    // 失败只告警——可用性优先，不给异常文件系统添启动故障
    if let Err(err) = restrict_to_owner(sing_box_home) {
        warn!(error = %err, path = ?sing_box_home, "Failed to restrict sing-box home permissions");
    }

    let sing_box_path = sing_box_home.join(sing_box_file_name());

    // 每次启动都删除并重新释放内嵌文件,保证与当前运行的二进制一致:
    // install.sh 升级、手动替换二进制等路径不经过面板自升级的清理逻辑。
    // 先删再写而非覆盖写:若有上次崩溃残留的 sing-box 进程仍在运行,覆盖写会得到 ETXTBSY。
    // 其余运行时文件(cache.db / config.json.cache)有意保留。
    let embedded_files: [(&str, &[u8]); 3] = [
        (sing_box_file_name(), SING_BOX_BINARY),
        ("chinaip.srs", IP_RULE_BINARY),
        ("chinasite.srs", SITE_RULE_BINARY),
    ];

    for (name, bytes) in embedded_files {
        let path = sing_box_home.join(name);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| map_remove_embedded_error(name, e))?;
        }
        info!("Extracting embedded file to {:?}", path);
        fs::write(&path, bytes)
            .map_err(|e| AppError::context(format!("Failed to write embedded file {name}"), e))?;
    }
    // 去广告功能已移除：清掉旧版本释放的广告规则集，避免孤儿文件常驻运行时目录
    let _ = fs::remove_file(sing_box_home.join("adblock_reject.srs"));
    set_executable(&sing_box_path)
        .map_err(|e| AppError::context("Failed to set permissions on sing-box binary", e))?;

    let dashboard_dir = sing_box_home.join("dashboard");
    if !dashboard_dir.exists() {
        fs::create_dir_all(&dashboard_dir)
            .map_err(|e| AppError::context("Failed to create sing-box dashboard directory", e))?;
    }

    Ok(sing_box_home.to_path_buf())
}

/// 在停止运行中的实例前验证 sing-box 配置，避免不必要的服务中断。
pub async fn validate_sing_box_config(
    state: &AppState,
    config_path: &std::path::Path,
) -> AppResult<()> {
    let sing_box_home = &state.runtime_paths.runtime_dir;
    let sing_box_path = sing_box_home.join(sing_box_file_name());

    let output = tokio::process::Command::new(&sing_box_path)
        .current_dir(sing_box_home)
        .arg("check")
        .arg("-c")
        .arg(config_path)
        .output()
        .await
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
    // Every first-start path (cache, snapshot, manuals, recover/activate,
    // REST start) shares this so a subscription-only OpenWrt boot still
    // installs kmod-tun / kmod-nft-queue. Non-OpenWrt returns immediately.
    #[cfg(not(windows))]
    if let Err(err) = crate::services::openwrt::check_and_install_openwrt_dependencies().await {
        error!(error = %err, "Failed to check or install OpenWrt dependencies");
    }

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
        state.runtime_ready.store(false, Ordering::Relaxed);
        state.set_runtime_phase(RuntimePhase::Starting);
        // Retire any watcher from the previous start before this spawn begins.
        state.sing_generation.fetch_add(1, Ordering::Relaxed) + 1
    };

    if let Err(err) = spawn_and_probe_sing_box(state, generation).await {
        state.runtime_ready.store(false, Ordering::Relaxed);
        state.set_runtime_phase(RuntimePhase::Failed);
        return Err(err);
    }
    state.runtime_ready.store(true, Ordering::Relaxed);
    state.set_runtime_phase(RuntimePhase::Ready);
    spawn_crash_watcher(state.clone(), generation);
    clear_kernel_give_up_warning(state).await;
    Ok(())
}

/// Reload the active config in place on Unix. sing-box officially wires
/// SIGHUP to its reload path; miao still verifies that the same process stays
/// alive and the data plane becomes healthy before publishing `ready` again.
#[cfg(unix)]
pub async fn reload_sing_internal(state: &Arc<AppState>) -> AppResult<()> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;

    state.runtime_ready.store(false, Ordering::Relaxed);
    state.set_runtime_phase(RuntimePhase::Reloading);

    let (pid, generation) = {
        let mut lock = state.sing_process.lock().await;
        let Some(process) = lock.as_mut() else {
            state.set_runtime_phase(RuntimePhase::Failed);
            return Err(AppError::message("sing-box is not running"));
        };
        let exit_status = match process.child.try_wait() {
            Ok(status) => status,
            Err(err) => {
                state.set_runtime_phase(RuntimePhase::Failed);
                return Err(AppError::context(
                    "Failed to check sing-box before reload",
                    err,
                ));
            }
        };
        if let Some(status) = exit_status {
            *lock = None;
            state.set_runtime_phase(RuntimePhase::Failed);
            return Err(AppError::message(format!(
                "sing-box exited before reload with code {}",
                status.code().unwrap_or(-1)
            )));
        }
        let Some(pid) = process.child.id() else {
            state.set_runtime_phase(RuntimePhase::Failed);
            return Err(AppError::message("sing-box process ID is unavailable"));
        };
        // Retire the previous watcher while preserving the child itself. A
        // fresh watcher is attached only after reload health is established.
        let generation = state.sing_generation.fetch_add(1, Ordering::Relaxed) + 1;
        (pid, generation)
    };

    if let Err(err) = kill(Pid::from_raw(pid as i32), Signal::SIGHUP) {
        if is_sing_box_running(state).await {
            state.runtime_ready.store(true, Ordering::Relaxed);
            state.set_runtime_phase(RuntimePhase::Ready);
            spawn_crash_watcher(state.clone(), generation);
        } else {
            state.runtime_ready.store(false, Ordering::Relaxed);
            state.set_runtime_phase(RuntimePhase::Failed);
        }
        return Err(AppError::message(format!(
            "Failed to signal sing-box reload: {err}"
        )));
    }

    if let Err(err) = wait_for_sing_box_reload_ready(state, generation, pid).await {
        state.runtime_ready.store(false, Ordering::Relaxed);
        state.set_runtime_phase(RuntimePhase::Failed);
        return Err(err);
    }

    state.runtime_ready.store(true, Ordering::Relaxed);
    state.set_runtime_phase(RuntimePhase::Ready);
    spawn_crash_watcher(state.clone(), generation);
    clear_kernel_give_up_warning(state).await;
    info!(pid, "sing-box configuration reloaded in place");
    Ok(())
}

#[cfg(all(unix, not(test)))]
async fn wait_for_sing_box_reload_ready(
    state: &Arc<AppState>,
    expected_generation: u64,
    expected_pid: u32,
) -> AppResult<()> {
    const PROBE_INTERVAL: Duration = Duration::from_millis(25);
    const RELOAD_SETTLE_TIME: Duration = Duration::from_millis(500);
    const RELOAD_TIMEOUT: Duration = Duration::from_secs(8);
    const CLASH_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

    let started = Instant::now();
    let mut consecutive_ready = 0u8;
    loop {
        sleep(PROBE_INTERVAL).await;
        let mut lock = state.sing_process.lock().await;
        if !start_still_current(state, expected_generation) {
            return Err(AppError::message("sing-box reload was cancelled"));
        }
        let Some(process) = lock.as_mut() else {
            return Err(AppError::message("sing-box exited during reload"));
        };
        if process.child.id() != Some(expected_pid) {
            return Err(AppError::message(
                "sing-box process changed unexpectedly during reload",
            ));
        }
        if let Some(status) = process
            .child
            .try_wait()
            .map_err(|e| AppError::context("Failed to check sing-box reload status", e))?
        {
            *lock = None;
            return Err(AppError::message(format!(
                "sing-box exited during reload with code {}",
                status.code().unwrap_or(-1)
            )));
        }
        drop(lock);

        let tun_ready = {
            #[cfg(target_os = "linux")]
            {
                std::path::Path::new("/sys/class/net/sing-tun").exists()
            }
            #[cfg(not(target_os = "linux"))]
            {
                true
            }
        };
        let clash_ready = if started.elapsed() >= RELOAD_SETTLE_TIME {
            state
                .http_client
                .get(clash_api_url("/version"))
                .timeout(CLASH_PROBE_TIMEOUT)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        } else {
            false
        };

        if tun_ready && clash_ready {
            consecutive_ready += 1;
            if consecutive_ready >= 2 {
                return Ok(());
            }
        } else {
            consecutive_ready = 0;
        }

        if started.elapsed() >= RELOAD_TIMEOUT {
            return Err(AppError::message(
                "sing-box data plane did not become ready within 8 seconds after reload",
            ));
        }
    }
}

#[cfg(all(unix, test))]
async fn wait_for_sing_box_reload_ready(
    state: &Arc<AppState>,
    expected_generation: u64,
    expected_pid: u32,
) -> AppResult<()> {
    sleep(Duration::from_millis(50)).await;
    let mut lock = state.sing_process.lock().await;
    if !start_still_current(state, expected_generation) {
        return Err(AppError::message("sing-box reload was cancelled"));
    }
    let Some(process) = lock.as_mut() else {
        return Err(AppError::message("sing-box exited during reload"));
    };
    if process.child.id() != Some(expected_pid) {
        return Err(AppError::message(
            "sing-box process changed unexpectedly during reload",
        ));
    }
    if let Some(status) = process
        .child
        .try_wait()
        .map_err(|e| AppError::context("Failed to check sing-box reload status", e))?
    {
        *lock = None;
        return Err(AppError::message(format!(
            "sing-box exited during reload with code {}",
            status.code().unwrap_or(-1)
        )));
    }
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

    #[cfg(windows)]
    {
        cleanup_stale_tun_adapter();
        ensure_hidden_console();
    }

    let mut command = tokio::process::Command::new(&sing_box_path);
    command
        .current_dir(sing_box_home)
        .arg("run")
        .arg("-c")
        .arg(config_path);

    #[cfg(windows)]
    command.creation_flags(WINDOWS_CREATE_NEW_PROCESS_GROUP);
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

    #[cfg(windows)]
    assign_child_to_kernel_job(&child);

    let pid = child.id();
    info!(pid = pid, "sing-box process spawned");

    // Own the child before the first await so cancelling init cannot drop it
    // untracked; stop_sing_internal then finds it.
    *lock = Some(SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    drop(lock);

    wait_for_sing_box_ready(state, expected_generation).await
}

#[cfg(not(test))]
async fn wait_for_sing_box_ready(state: &Arc<AppState>, expected_generation: u64) -> AppResult<()> {
    const PROBE_INTERVAL: Duration = Duration::from_millis(25);
    const MIN_STABLE_TIME: Duration = Duration::from_millis(100);
    // 3s was enough on desktops; OpenWrt auto_redirect + a slow Clash bind
    // can miss that window and get the child killed while it is still coming up.
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
    const CLASH_PROBE_TIMEOUT: Duration = Duration::from_millis(250);

    let started = Instant::now();
    let mut consecutive_ready = 0u8;
    loop {
        sleep(PROBE_INTERVAL).await;
        let mut lock = state.sing_process.lock().await;
        if !start_still_current(state, expected_generation) {
            return Err(AppError::message("sing-box start was cancelled"));
        }
        let Some(proc) = lock.as_mut() else {
            return Err(AppError::message("sing-box start was cancelled"));
        };
        if let Some(exit_status) = proc
            .child
            .try_wait()
            .map_err(|e| AppError::context("Failed to check sing-box startup status", e))?
        {
            *lock = None;
            #[cfg(windows)]
            cleanup_stale_tun_adapter();
            let code = exit_status.code().unwrap_or(-1);
            return Err(AppError::message(format!(
                "sing-box exited during startup with code {code}"
            )));
        }
        drop(lock);

        let tun_ready = {
            #[cfg(target_os = "linux")]
            {
                std::path::Path::new("/sys/class/net/sing-tun").exists()
            }
            #[cfg(not(target_os = "linux"))]
            {
                true
            }
        };
        let clash_ready = if started.elapsed() >= MIN_STABLE_TIME {
            state
                .http_client
                .get(clash_api_url("/version"))
                .timeout(CLASH_PROBE_TIMEOUT)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        } else {
            false
        };

        if tun_ready && clash_ready {
            consecutive_ready += 1;
            if consecutive_ready >= 2 {
                info!(
                    elapsed_ms = started.elapsed().as_millis(),
                    "sing-box data plane is ready"
                );
                return Ok(());
            }
        } else {
            consecutive_ready = 0;
        }

        if started.elapsed() >= STARTUP_TIMEOUT {
            terminate_failed_start(state, expected_generation).await;
            return Err(AppError::message(
                "sing-box process started but its data plane did not become ready within 8 seconds",
            ));
        }
    }
}

/// Hermetic transaction tests use a minimal fake child without a Clash API or
/// TUN device. Production builds use the data-plane probe above.
#[cfg(test)]
async fn wait_for_sing_box_ready(state: &Arc<AppState>, expected_generation: u64) -> AppResult<()> {
    sleep(Duration::from_millis(50)).await;
    let mut lock = state.sing_process.lock().await;
    if !start_still_current(state, expected_generation) {
        return Err(AppError::message("sing-box start was cancelled"));
    }
    let Some(proc) = lock.as_mut() else {
        return Err(AppError::message("sing-box start was cancelled"));
    };
    if let Some(exit_status) = proc
        .child
        .try_wait()
        .map_err(|e| AppError::context("Failed to check sing-box startup status", e))?
    {
        *lock = None;
        return Err(AppError::message(format!(
            "sing-box exited during startup with code {}",
            exit_status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

async fn terminate_failed_start(state: &Arc<AppState>, expected_generation: u64) {
    let mut lock = state.sing_process.lock().await;
    if state.sing_generation.load(Ordering::Relaxed) != expected_generation {
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
    state.runtime_ready.store(false, Ordering::Relaxed);
    state.set_runtime_phase(RuntimePhase::Stopping);
    let mut lock = state.sing_process.lock().await;
    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait().ok().flatten().is_none() {
            request_graceful_exit(&mut proc.child).await;
        }
    }
    *lock = None;
    // 让正在监护的崩溃看门狗退出：这是一次有意停止。
    state.sing_generation.fetch_add(1, Ordering::Relaxed);
    state.set_runtime_phase(RuntimePhase::Stopped);
}

/// 崩溃看门狗的巡检间隔。测试里缩短以保持用例快速。
#[cfg(not(test))]
const KERNEL_WATCH_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(test)]
const KERNEL_WATCH_INTERVAL: Duration = Duration::from_millis(200);

/// 连续自动重启超过此次数后放弃，并在面板上告警。
const MAX_KERNEL_RESTARTS: u32 = 5;

/// 内核存活超过该时长则认为已稳定，重置重启计数。
const KERNEL_STABLE_AFTER: Duration = Duration::from_secs(60);

const KERNEL_GIVE_UP_WARNING: &str = "sing-box 反复异常退出，已停止自动拉起。请检查配置或查看日志";

fn restart_backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << (attempt.saturating_sub(1)).min(4))
}

fn spawn_crash_watcher(state: Arc<AppState>, generation: u64) {
    tokio::spawn(watch_sing_box(state, generation));
}

fn start_still_current(state: &AppState, expected_generation: u64) -> bool {
    state.sing_generation.load(Ordering::Relaxed) == expected_generation
        && state.service_should_run.load(Ordering::Relaxed)
}

async fn clear_kernel_give_up_warning(state: &Arc<AppState>) {
    let mut warning = state.config_warning.lock().await;
    if warning.as_deref() == Some(KERNEL_GIVE_UP_WARNING) {
        *warning = None;
    }
}

/// 监护一次 sing-box 启动：异常退出时按退避自动拉起（复用当前 config.json，
/// 不重新生成）。有意停核/重启会递增 generation，看门狗随即退出。
async fn watch_sing_box(state: Arc<AppState>, generation: u64) {
    let mut restarts = 0u32;
    loop {
        sleep(KERNEL_WATCH_INTERVAL).await;
        if state.sing_generation.load(Ordering::Relaxed) != generation {
            return;
        }

        let crashed = {
            let mut lock = state.sing_process.lock().await;
            match lock.as_mut() {
                Some(proc) => match proc.child.try_wait() {
                    Ok(None) => {
                        if proc.started_at.elapsed() >= KERNEL_STABLE_AFTER {
                            restarts = 0;
                        }
                        false
                    }
                    Ok(Some(status)) => {
                        warn!(exit_code = ?status.code(), "sing-box exited unexpectedly");
                        *lock = None;
                        state.runtime_ready.store(false, Ordering::Relaxed);
                        true
                    }
                    Err(err) => {
                        warn!(error = %err, "Failed to poll sing-box process state");
                        *lock = None;
                        state.runtime_ready.store(false, Ordering::Relaxed);
                        true
                    }
                },
                // 状态轮询等路径可能先收割了已退出的进程：槽位为空且
                // generation 未变，仍视为一次异常退出。
                None => true,
            }
        };
        if !crashed {
            continue;
        }
        if state.sing_generation.load(Ordering::Relaxed) != generation
            || !state.service_should_run.load(Ordering::Relaxed)
        {
            return;
        }

        restarts += 1;
        if restarts > MAX_KERNEL_RESTARTS {
            error!("sing-box kept crashing; giving up on automatic restarts");
            state.runtime_ready.store(false, Ordering::Relaxed);
            state.set_runtime_phase(RuntimePhase::Failed);
            *state.config_warning.lock().await = Some(KERNEL_GIVE_UP_WARNING.to_string());
            return;
        }

        state.set_runtime_phase(RuntimePhase::Starting);
        sleep(restart_backoff(restarts)).await;
        if state.sing_generation.load(Ordering::Relaxed) != generation
            || !state.service_should_run.load(Ordering::Relaxed)
        {
            return;
        }

        // Serialize crash recovery with user-driven config transactions. If a
        // settings update wins the lock during backoff, its new generation
        // retires this watcher before it can spawn the old bytes concurrently.
        let _config_update = state.config_update.lock().await;
        if state.sing_generation.load(Ordering::Relaxed) != generation
            || !state.service_should_run.load(Ordering::Relaxed)
        {
            return;
        }

        match spawn_and_probe_sing_box(&state, generation).await {
            Ok(()) => {
                state.runtime_ready.store(true, Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Ready);
                info!(restarts, "sing-box restarted after an unexpected exit");
            }
            Err(err) => {
                state.runtime_ready.store(false, Ordering::Relaxed);
                warn!(error = %err, "Failed to restart sing-box after an unexpected exit")
            }
        }
    }
}

/// unix：运行时目录设为 0700（内含订阅凭证，仅属主可入）。
/// 其他平台无需处理：Windows 的 %TEMP% 本就是用户私有目录。
fn restrict_to_owner(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn set_executable(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

async fn request_graceful_exit(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    {
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

    #[cfg(windows)]
    {
        // CREATE_NEW_PROCESS_GROUP child; CTRL_BREAK reaches that group only.
        // Go maps CTRL_BREAK to os.Interrupt so sing-box can close WinTun.
        if let Some(pid) = child.id() {
            send_ctrl_break_to_group(pid);
        }
        match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
            Ok(Ok(_)) => restore_ctrl_handler(),
            _ => {
                restore_ctrl_handler();
                let _ = child.start_kill();
                let _ = child.wait().await;
                cleanup_stale_tun_adapter();
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}

/// Give this process a hidden console so a GUI parent can still deliver Ctrl+C
/// to sing-box. No-op when a console is already attached (CLI in a terminal).
#[cfg(windows)]
pub(crate) fn ensure_hidden_console() {
    use windows_sys::Win32::System::Console::{AllocConsole, GetConsoleWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

    unsafe {
        if GetConsoleWindow().is_null() && AllocConsole() != 0 {
            let hwnd = GetConsoleWindow();
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}

#[cfg(windows)]
fn send_ctrl_break_to_group(pid: u32) {
    use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, SetConsoleCtrlHandler};

    unsafe {
        SetConsoleCtrlHandler(None, 1);
        let _ = GenerateConsoleCtrlEvent(WINDOWS_CTRL_BREAK_EVENT, pid);
    }
}

#[cfg(windows)]
fn restore_ctrl_handler() {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    unsafe {
        SetConsoleCtrlHandler(None, 0);
    }
}

fn map_remove_embedded_error(name: &str, err: std::io::Error) -> AppError {
    if is_windows_sharing_violation(&err) {
        AppError::message(format!(
            "无法更新内核文件 {name}：残留的 sing-box 仍在运行。请结束该进程后重试。"
        ))
    } else {
        AppError::context(format!("Failed to remove stale embedded file {name}"), err)
    }
}

/// ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION: the previous kernel still
/// has the exe mapped.
fn is_windows_sharing_violation(err: &std::io::Error) -> bool {
    matches!(err.raw_os_error(), Some(32) | Some(33))
}

/// CREATE_NEW_PROCESS_GROUP. The child is its own group so CTRL_BREAK can
/// target that pid instead of broadcasting CTRL_C on the hidden console.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WINDOWS_CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// CTRL_BREAK_EVENT. Go treats this like CTRL_C (`os.Interrupt`).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WINDOWS_CTRL_BREAK_EVENT: u32 = 0x0000_0001;

/// JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE. Closing miao.exe kills assigned children.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const WINDOWS_JOB_KILL_ON_CLOSE: u32 = 0x0000_2000;

#[cfg(windows)]
fn kernel_job_handle() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use std::sync::OnceLock;
    static JOB: OnceLock<isize> = OnceLock::new();
    let raw = *JOB.get_or_init(|| {
        create_kill_on_close_job()
            .map(|handle| handle as isize)
            .unwrap_or(0)
    });
    if raw == 0 {
        None
    } else {
        Some(raw as windows_sys::Win32::Foundation::HANDLE)
    }
}

#[cfg(windows)]
fn create_kill_on_close_job() -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    };

    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return None;
    }

    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = WINDOWS_JOB_KILL_ON_CLOSE;
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&info) as u32,
        )
    };
    if ok == 0 {
        unsafe {
            CloseHandle(job);
        }
        return None;
    }
    Some(job)
}

#[cfg(windows)]
fn assign_child_to_kernel_job(child: &tokio::process::Child) {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let Some(job) = kernel_job_handle() else {
        tracing::warn!("No kernel job object; leftover sing-box will not die with this process");
        return;
    };
    let Some(process) = child.raw_handle() else {
        tracing::warn!("sing-box process handle is unavailable");
        return;
    };
    if unsafe { AssignProcessToJobObject(job, process as HANDLE) } == 0 {
        tracing::warn!("Failed to assign sing-box to kernel job object");
    }
}

#[cfg(test)]
const TUN_ADAPTER_NAME: &str = "sing-tun";

/// Forced TerminateProcess leaves WinTun attached; remove only `sing-tun`.
#[cfg(any(windows, test))]
pub(crate) fn tun_adapter_cleanup_command() -> (&'static str, Vec<&'static str>) {
    (
        "powershell.exe",
        vec![
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-NetAdapter -Name 'sing-tun' -ErrorAction SilentlyContinue | Remove-NetAdapter -Confirm:$false",
        ],
    )
}

#[cfg(windows)]
fn cleanup_stale_tun_adapter() {
    let (program, args) = tun_adapter_cleanup_command();
    match std::process::Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => {
            info!("Removed leftover sing-tun adapter if it was present");
        }
        Ok(output) => {
            tracing::warn!(
                status = ?output.status,
                "sing-tun adapter cleanup returned non-success"
            );
        }
        Err(err) => {
            tracing::warn!(error = %err, "Failed to run sing-tun adapter cleanup");
        }
    }
}

#[cfg(test)]
mod tests {
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
}
