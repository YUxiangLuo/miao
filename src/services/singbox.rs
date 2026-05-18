#[cfg(unix)]
use nix::sys::signal::{kill, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::info;

use crate::error::{AppError, AppResult};
use crate::state::{AppState, SingBoxProcess};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const SING_BOX_BINARY: &[u8] = include_bytes!("../../embedded/sing-box-amd64");

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const SING_BOX_BINARY: &[u8] = include_bytes!("../../embedded/sing-box-arm64");

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const SING_BOX_BINARY: &[u8] = include_bytes!("../../embedded/sing-box-windows-amd64.exe");

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "windows", target_arch = "x86_64"),
)))]
compile_error!(
    "Unsupported target: supported targets are linux x86_64/aarch64 and windows x86_64."
);

const IP_RULE_BINARY: &[u8] = include_bytes!("../../embedded/geoip-cn.srs");
const SITE_RULE_BINARY: &[u8] = include_bytes!("../../embedded/geosite-geolocation-cn.srs");

pub fn get_sing_box_home() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::temp_dir().join("miao-sing-box")
    } else {
        PathBuf::from("/tmp/miao-sing-box")
    }
}

fn sing_box_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "sing-box.exe"
    } else {
        "sing-box"
    }
}

fn set_executable_permissions(path: &Path) -> AppResult<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|e| AppError::context("Failed to set permissions on sing-box binary", e))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

pub fn extract_sing_box() -> AppResult<PathBuf> {
    let sing_box_home = get_sing_box_home();
    if !sing_box_home.exists() {
        fs::create_dir_all(&sing_box_home)
            .map_err(|e| AppError::context("Failed to create sing-box home directory", e))?;
    }

    let sing_box_path = sing_box_home.join(sing_box_file_name());
    let ip_rule_path = sing_box_home.join("chinaip.srs");
    let site_rule_path = sing_box_home.join("chinasite.srs");

    if !sing_box_path.exists() {
        info!("Extracting embedded sing-box binary to {:?}", sing_box_path);
        fs::write(&sing_box_path, SING_BOX_BINARY)
            .map_err(|e| AppError::context("Failed to write embedded sing-box binary", e))?;
        set_executable_permissions(&sing_box_path)?;
        info!("sing-box binary extracted successfully");
    }

    if !ip_rule_path.exists() {
        info!("Extracting geoip rule file to {:?}", ip_rule_path);
        fs::write(&ip_rule_path, IP_RULE_BINARY)
            .map_err(|e| AppError::context("Failed to write geoip rule file", e))?;
    }
    if !site_rule_path.exists() {
        info!("Extracting geosite rule file to {:?}", site_rule_path);
        fs::write(&site_rule_path, SITE_RULE_BINARY)
            .map_err(|e| AppError::context("Failed to write geosite rule file", e))?;
    }

    let dashboard_dir = sing_box_home.join("dashboard");
    if !dashboard_dir.exists() {
        fs::create_dir_all(&dashboard_dir)
            .map_err(|e| AppError::context("Failed to create sing-box dashboard directory", e))?;
    }

    Ok(sing_box_home)
}

/// 在停止运行中的实例前验证 sing-box 配置，避免不必要的服务中断
pub async fn validate_sing_box_config() -> AppResult<()> {
    let sing_box_home = get_sing_box_home();
    let sing_box_path = sing_box_home.join(sing_box_file_name());
    let config_path = sing_box_home.join("config.json");

    let output = tokio::process::Command::new(&sing_box_path)
        .current_dir(&sing_box_home)
        .arg("check")
        .arg("-c")
        .arg(&config_path)
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

    let sing_box_home = get_sing_box_home();
    let sing_box_path = sing_box_home.join(sing_box_file_name());
    let config_path = sing_box_home.join("config.json");

    info!(binary = ?sing_box_path, config = ?config_path, "Starting sing-box");

    let mut child = tokio::process::Command::new(&sing_box_path)
        .current_dir(&sing_box_home)
        .arg("run")
        .arg("-c")
        .arg(&config_path)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| AppError::context("Failed to spawn sing-box process", e))?;

    let pid = child.id();
    info!(pid = pid, "sing-box process spawned");

    sleep(Duration::from_millis(500)).await;
    if let Some(exit_status) = child
        .try_wait()
        .map_err(|e| AppError::context("Failed to check sing-box startup status", e))?
    {
        let code = exit_status.code().unwrap_or(-1);
        return Err(AppError::message(format!(
            "sing-box exited immediately with code {}",
            code
        )));
    }

    *lock = Some(SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    drop(lock);

    Ok(())
}

pub async fn stop_sing_internal(state: &Arc<AppState>) {
    let mut lock = state.sing_process.lock().await;
    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait().ok().flatten().is_none() {
            terminate_child(&mut proc.child).await;
        }
    }
    *lock = None;
}

#[cfg(unix)]
async fn terminate_child(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        // 发送 SIGTERM 信号请求进程优雅退出
        let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);

        // 使用 timeout 等待进程退出，避免忙等待
        let wait_result = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;

        match wait_result {
            Ok(Ok(_)) => {
                // 进程正常退出
            }
            _ => {
                // 超时或等待失败，强制杀死进程
                let _ = child.start_kill();
                let _ = child.wait().await;
            }
        }
    }
}

#[cfg(not(unix))]
async fn terminate_child(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}
