use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::info;

use crate::error::{AppError, AppResult};
use crate::services::geoip;
use crate::state::{AppState, SingBoxProcess};

#[cfg(target_arch = "x86_64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../../embedded/sing-box-amd64");

#[cfg(target_arch = "aarch64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../../embedded/sing-box-arm64");

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("Unsupported architecture: only x86_64 and aarch64 are supported. Please add support for your target architecture in embedded/ directory.");

const IP_RULE_BINARY: &[u8] = include_bytes!("../../embedded/geoip-cn.srs");
const SITE_RULE_BINARY: &[u8] = include_bytes!("../../embedded/geosite-geolocation-cn.srs");
const ADBLOCK_RULE_BINARY: &[u8] = include_bytes!("../../embedded/adblock_reject.srs");
const GEO_DB_BINARY: &[u8] = include_bytes!("../../embedded/geoip-city.mmdb");

pub fn get_sing_box_home() -> PathBuf {
    PathBuf::from("/tmp/miao-sing-box")
}

pub fn extract_sing_box() -> AppResult<PathBuf> {
    let sing_box_home = get_sing_box_home();
    if !sing_box_home.exists() {
        fs::create_dir_all(&sing_box_home)
            .map_err(|e| AppError::context("Failed to create sing-box home directory", e))?;
    }

    let sing_box_path = sing_box_home.join("sing-box");

    // 每次启动都删除并重新释放内嵌文件,保证与当前运行的二进制一致:
    // install.sh 升级、手动替换二进制等路径不经过面板自升级的清理逻辑。
    // 先删再写而非覆盖写:若有上次崩溃残留的 sing-box 进程仍在运行,覆盖写会得到 ETXTBSY。
    // 其余运行时文件(cache.db / config.json.cache / .last_proxy)有意保留。
    let embedded_files: [(&str, &[u8]); 5] = [
        ("sing-box", SING_BOX_BINARY),
        ("chinaip.srs", IP_RULE_BINARY),
        ("chinasite.srs", SITE_RULE_BINARY),
        ("adblock_reject.srs", ADBLOCK_RULE_BINARY),
        (geoip::GEO_DB_FILENAME, GEO_DB_BINARY),
    ];

    for (name, bytes) in embedded_files {
        let path = sing_box_home.join(name);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                AppError::context(format!("Failed to remove stale embedded file {name}"), e)
            })?;
        }
        info!("Extracting embedded file to {:?}", path);
        fs::write(&path, bytes)
            .map_err(|e| AppError::context(format!("Failed to write embedded file {name}"), e))?;
    }
    fs::set_permissions(&sing_box_path, fs::Permissions::from_mode(0o755))
        .map_err(|e| AppError::context("Failed to set permissions on sing-box binary", e))?;

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
    let sing_box_path = sing_box_home.join("sing-box");
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
    let sing_box_path = sing_box_home.join("sing-box");
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
            if let Some(pid) = proc.child.id() {
                // 发送 SIGTERM 信号请求进程优雅退出
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);

                // 使用 timeout 等待进程退出，避免忙等待
                let wait_result =
                    tokio::time::timeout(Duration::from_secs(3), proc.child.wait()).await;

                match wait_result {
                    Ok(Ok(_)) => {
                        // 进程正常退出
                    }
                    _ => {
                        // 超时或等待失败，强制杀死进程
                        let _ = proc.child.start_kill();
                        let _ = proc.child.wait().await;
                    }
                }
            }
        }
    }
    *lock = None;
}
