//! Windows 开机自启：任务计划程序实现。
//! 勾选状态 = 任务是否存在，不落配置文件。任务以当前用户身份、最高权限
//! （/RL HIGHEST）在登录时运行，因此自启不再弹 UAC；命令行带 `--minimized`
//! 让窗口先进托盘，不在登录时糊脸。

use crate::error::{AppError, AppResult};
#[cfg(any(windows, test))]
use std::path::Path;

/// 任务计划里的任务名。
#[cfg_attr(not(windows), allow(dead_code))]
pub const AUTOSTART_TASK_NAME: &str = "Miao";

/// 自启时传给 miao.exe 的参数：窗口先不显示，缩进托盘。
pub const MINIMIZED_ARG: &str = "--minimized";

/// CREATE_NO_WINDOW：GUI 进程拉控制台子进程时不闪黑窗。
#[cfg_attr(not(windows), allow(dead_code))]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn is_enabled() -> bool {
    #[cfg(windows)]
    {
        let (program, args) = query_command();
        run_hidden(&program, &args)
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn set_enabled(enabled: bool) -> AppResult<()> {
    #[cfg(windows)]
    {
        let (program, args) = if enabled {
            let exe = std::env::current_exe()
                .map_err(|e| AppError::context("Failed to locate current exe", e))?;
            create_command(&exe)
        } else {
            delete_command()
        };
        let status = run_hidden(&program, &args)
            .map_err(|e| AppError::context("Failed to run schtasks", e))?;
        if !status.success() {
            return Err(AppError::message(format!("schtasks exited with {status}")));
        }
        // 回读校验：任务存在性就是状态本体，执行完必须与目标一致
        if is_enabled() != enabled {
            return Err(AppError::message("schtasks 执行后自启状态未生效"));
        }
        tracing::info!(enabled, "autostart toggled");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err(AppError::message("当前平台不支持开机自启"))
    }
}

#[cfg(windows)]
fn run_hidden(program: &str, args: &[String]) -> std::io::Result<std::process::ExitStatus> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
}

#[cfg(any(windows, test))]
pub(crate) fn query_command() -> (String, Vec<String>) {
    (
        "schtasks.exe".into(),
        vec!["/Query".into(), "/TN".into(), AUTOSTART_TASK_NAME.into()],
    )
}

#[cfg(any(windows, test))]
pub(crate) fn create_command(exe: &Path) -> (String, Vec<String>) {
    (
        "schtasks.exe".into(),
        vec![
            "/Create".into(),
            "/TN".into(),
            AUTOSTART_TASK_NAME.into(),
            "/TR".into(),
            format!("\"{}\" {}", exe.display(), MINIMIZED_ARG),
            "/SC".into(),
            "ONLOGON".into(),
            "/RL".into(),
            "HIGHEST".into(),
            "/F".into(),
        ],
    )
}

#[cfg(any(windows, test))]
pub(crate) fn delete_command() -> (String, Vec<String>) {
    (
        "schtasks.exe".into(),
        vec![
            "/Delete".into(),
            "/TN".into(),
            AUTOSTART_TASK_NAME.into(),
            "/F".into(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::{create_command, delete_command, is_enabled, query_command, set_enabled};
    use std::path::PathBuf;

    #[test]
    fn create_command_runs_elevated_at_logon_minimized() {
        let exe = PathBuf::from(r"C:\Users\Jane Doe\AppData\Local\Miao\miao.exe");
        let (program, args) = create_command(&exe);
        assert_eq!(program, "schtasks.exe");
        let joined = args.join(" ");
        assert!(joined.contains("/SC ONLOGON"));
        assert!(joined.contains("/RL HIGHEST"));
        assert!(joined.contains("/F"));
        let tr = args
            .windows(2)
            .find(|pair| pair[0] == "/TR")
            .map(|pair| pair[1].clone())
            .expect("/TR argument");
        // 路径含空格必须带引号，且附带 --minimized
        assert_eq!(
            tr,
            format!("\"{}\" --minimized", exe.display())
        );
    }

    #[test]
    fn query_and_delete_target_the_task_name() {
        let (query_program, query_args) = query_command();
        let (delete_program, delete_args) = delete_command();
        assert_eq!(query_program, "schtasks.exe");
        assert_eq!(delete_program, "schtasks.exe");
        assert!(query_args.join(" ").contains("/TN Miao"));
        let delete_joined = delete_args.join(" ");
        assert!(delete_joined.contains("/Delete"));
        assert!(delete_joined.contains("/TN Miao"));
        assert!(delete_joined.contains("/F"));
    }

    #[cfg(not(windows))]
    #[test]
    fn autostart_is_unsupported_off_windows() {
        assert!(!is_enabled());
        assert!(set_enabled(true).is_err());
        assert!(set_enabled(false).is_err());
    }
}
