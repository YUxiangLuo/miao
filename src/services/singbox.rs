use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};

use crate::error::{AppError, AppResult};
use crate::state::{AppState, SingBoxProcess};

#[cfg(target_arch = "x86_64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../../embedded/sing-box-amd64");

#[cfg(target_arch = "aarch64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../../embedded/sing-box-arm64");

const IP_RULE_BINARY: &[u8] = include_bytes!("../../embedded/geoip-cn.srs");
const SITE_RULE_BINARY: &[u8] = include_bytes!("../../embedded/geosite-geolocation-cn.srs");

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
    let ip_rule_path = sing_box_home.join("chinaip.srs");
    let site_rule_path = sing_box_home.join("chinasite.srs");

    if !sing_box_path.exists() {
        println!("Extracting embedded sing-box binary to {:?}", sing_box_path);
        fs::write(&sing_box_path, SING_BOX_BINARY)
            .map_err(|e| AppError::context("Failed to write embedded sing-box binary", e))?;
        fs::set_permissions(&sing_box_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| AppError::context("Failed to set permissions on sing-box binary", e))?;
        println!("sing-box binary extracted successfully");
    }

    if !ip_rule_path.exists() {
        println!("Extracting geoip rule file to {:?}", ip_rule_path);
        fs::write(&ip_rule_path, IP_RULE_BINARY)
            .map_err(|e| AppError::context("Failed to write geoip rule file", e))?;
    }
    if !site_rule_path.exists() {
        println!("Extracting geosite rule file to {:?}", site_rule_path);
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

pub async fn start_sing_internal(state: &Arc<AppState>) -> AppResult<()> {
    let mut lock = state.sing_process.lock().await;
    if let Some(ref mut proc) = *lock {
        if proc
            .child
            .try_wait()
            .map_err(|e| AppError::context("Failed to check whether sing-box is already running", e))?
            .is_none()
        {
            return Err(AppError::AlreadyRunning);
        }
    }

    let sing_box_home = get_sing_box_home();
    let sing_box_path = sing_box_home.join("sing-box");
    let config_path = sing_box_home.join("config.json");

    println!("Starting sing-box from: {:?}", sing_box_path);
    println!("Using config: {:?}", config_path);

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
    println!("sing-box process spawned with PID: {:?}", pid);

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
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                for _ in 0..30 {
                    sleep(Duration::from_millis(100)).await;
                    if proc.child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                }
                if proc.child.try_wait().ok().flatten().is_none() {
                    proc.child.start_kill().ok();
                    let _ = proc.child.wait().await;
                }
            }
        }
    }
    *lock = None;
}
