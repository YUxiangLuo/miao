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

#[cfg(windows)]
fn run_hidden_output(program: &str, args: &[String]) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stderr(std::process::Stdio::null())
        .output()
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

#[cfg(windows)]
fn query_xml_command() -> (String, Vec<String>) {
    (
        "schtasks.exe".into(),
        vec![
            "/Query".into(),
            "/TN".into(),
            AUTOSTART_TASK_NAME.into(),
            "/XML".into(),
        ],
    )
}

/// schtasks /XML 重定向输出为 UTF-16LE（带 BOM）；兼容无 BOM 的情况。
#[cfg(any(windows, test))]
fn decode_task_xml(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// 从任务 XML 提取 <Command>（exe 路径）。走 /XML 而不是 /V /FO LIST：
/// 后者列名随系统语言变化，XML 结构与locale无关。
#[cfg(any(windows, test))]
fn extract_task_command(xml: &str) -> Option<String> {
    let start = xml.find("<Command>")? + "<Command>".len();
    let end = xml[start..].find("</Command>")? + start;
    let command = xml[start..end].trim();
    if command.is_empty() {
        None
    } else {
        Some(command.to_string())
    }
}

#[cfg(any(windows, test))]
fn same_exe_path(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('/', "\\").trim_matches('"').to_lowercase();
    norm(a) == norm(b)
}

/// 自修复：任务存在但指向的 exe 与当前进程不一致时（升级/迁移后旧任务残留，
/// 会把旧版本拉起来），用当前路径重注册。每次启动调用，幂等。
pub fn repair_if_stale() {
    #[cfg(windows)]
    {
        if !is_enabled() {
            return;
        }
        let current = match std::env::current_exe() {
            Ok(path) => path,
            Err(_) => return,
        };
        let (program, args) = query_xml_command();
        let output = match run_hidden_output(&program, &args) {
            Ok(output) if output.status.success() => output,
            _ => {
                tracing::warn!("autostart task XML query failed, skipping path check");
                return;
            }
        };
        match extract_task_command(&decode_task_xml(&output.stdout)) {
            Some(task_exe) if same_exe_path(&task_exe, &current.to_string_lossy()) => {}
            Some(task_exe) => {
                tracing::info!(
                    task_exe,
                    current = %current.display(),
                    "autostart task points at a stale exe, re-registering with current path"
                );
                if let Err(err) = set_enabled(true) {
                    tracing::warn!(error = %err, "autostart task repair failed");
                }
            }
            None => tracing::warn!("autostart task XML has no <Command>, skipping path check"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{create_command, delete_command, query_command};
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
        assert_eq!(tr, format!("\"{}\" --minimized", exe.display()));
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
        assert!(!super::is_enabled());
        assert!(super::set_enabled(true).is_err());
        assert!(super::set_enabled(false).is_err());
    }

    #[test]
    fn decode_task_xml_handles_utf16le_bom() {
        let text = "<?xml version=\"1.0\"?><Task></Task>";
        let mut bytes = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(super::decode_task_xml(&bytes), text);
        // 无 BOM 时按 UTF-8 兜底
        assert_eq!(super::decode_task_xml(text.as_bytes()), text);
    }

    #[test]
    fn extract_task_command_reads_command_element() {
        let xml = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task><Actions><Exec><Command>C:\Users\Jane\AppData\Local\Miao\miao.exe</Command><Arguments>--minimized</Arguments></Exec></Actions></Task>"#;
        assert_eq!(
            super::extract_task_command(xml).as_deref(),
            Some(r"C:\Users\Jane\AppData\Local\Miao\miao.exe")
        );
        assert_eq!(super::extract_task_command("<Task/>"), None);
        assert_eq!(super::extract_task_command("<Command></Command>"), None);
    }

    #[test]
    fn same_exe_path_is_case_and_slash_insensitive() {
        assert!(super::same_exe_path(
            r"C:\Users\Jane\Miao\miao.exe",
            r"c:\users\jane\miao\miao.exe"
        ));
        assert!(super::same_exe_path(
            "C:/Users/Jane/Miao/miao.exe",
            r"C:\Users\Jane\Miao\miao.exe"
        ));
        assert!(super::same_exe_path(
            r#""C:\Program Files\Miao\miao.exe""#,
            r"C:\Program Files\Miao\miao.exe"
        ));
        assert!(!super::same_exe_path(
            r"C:\old\miao.exe",
            r"C:\new\miao.exe"
        ));
    }
}
