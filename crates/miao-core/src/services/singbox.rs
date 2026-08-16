use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{sleep, Duration};
use tracing::info;

use crate::error::{AppError, AppResult};
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
const ADBLOCK_RULE_BINARY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../embedded/adblock_reject.srs"
));

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

pub fn extract_sing_box() -> AppResult<PathBuf> {
    let sing_box_home = get_sing_box_home();
    if !sing_box_home.exists() {
        fs::create_dir_all(&sing_box_home)
            .map_err(|e| AppError::context("Failed to create sing-box home directory", e))?;
    }

    let sing_box_path = sing_box_home.join(sing_box_file_name());

    // 每次启动都删除并重新释放内嵌文件,保证与当前运行的二进制一致:
    // install.sh 升级、手动替换二进制等路径不经过面板自升级的清理逻辑。
    // 先删再写而非覆盖写:若有上次崩溃残留的 sing-box 进程仍在运行,覆盖写会得到 ETXTBSY。
    // 其余运行时文件(cache.db / config.json.cache / .last_proxy)有意保留。
    let embedded_files: [(&str, &[u8]); 4] = [
        (sing_box_file_name(), SING_BOX_BINARY),
        ("chinaip.srs", IP_RULE_BINARY),
        ("chinasite.srs", SITE_RULE_BINARY),
        ("adblock_reject.srs", ADBLOCK_RULE_BINARY),
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
    set_executable(&sing_box_path)
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

    #[cfg(windows)]
    ensure_hidden_console();

    let mut command = tokio::process::Command::new(&sing_box_path);
    command
        .current_dir(&sing_box_home)
        .arg("run")
        .arg("-c")
        .arg(&config_path);

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

    let mut child = command
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
            request_graceful_exit(&mut proc.child).await;
        }
    }
    *lock = None;
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
        // CREATE_NO_WINDOW children cannot receive Ctrl+C (#3806). We inherit a
        // hidden console and send CTRL_C so sing-box can close WinTun itself.
        send_ctrl_c_to_console();
        match tokio::time::timeout(Duration::from_secs(3), child.wait()).await {
            Ok(Ok(_)) => restore_ctrl_c_handling(),
            _ => {
                restore_ctrl_c_handling();
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
fn send_ctrl_c_to_console() {
    use windows_sys::Win32::System::Console::{
        GenerateConsoleCtrlEvent, SetConsoleCtrlHandler, CTRL_C_EVENT,
    };

    unsafe {
        SetConsoleCtrlHandler(None, 1);
        let _ = GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0);
    }
}

#[cfg(windows)]
fn restore_ctrl_c_handling() {
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    unsafe {
        SetConsoleCtrlHandler(None, 0);
    }
}

#[cfg(test)]
const TUN_ADAPTER_NAME: &str = "sing-tun";

/// Command used after a forced Windows kill. Tests assert the shape only;
/// callers on Windows spawn it, Linux/CI never execute it.
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
    use super::{tun_adapter_cleanup_command, TUN_ADAPTER_NAME};

    #[test]
    fn tun_adapter_cleanup_targets_sing_tun_only() {
        let (program, args) = tun_adapter_cleanup_command();
        let joined = args.join(" ");
        assert_eq!(program, "powershell.exe");
        assert!(joined.contains(TUN_ADAPTER_NAME));
        assert!(joined.contains("Remove-NetAdapter"));
        assert!(!joined.contains("Get-NetAdapter -Name '*'"));
    }
}
