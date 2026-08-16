#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use miao_core::{
    acquire_single_instance, autostart_is_enabled, autostart_set_enabled, default_log_path,
    focus_existing_window, is_elevated, peek_single_instance, require_privileges,
    show_user_error, spawn_server, InstanceAcquire, InstancePeek, RuntimeOptions, ServerHandle,
    MINIMIZED_ARG,
};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

struct Panel(Mutex<Option<ServerHandle>>);

fn main() {
    match peek_single_instance() {
        InstancePeek::AlreadyRunning => {
            if !focus_existing_window() {
                show_user_error("Miao", "Miao 已在运行");
            }
            return;
        }
        InstancePeek::Failed => {
            show_user_error("Miao", "无法检查是否已在运行");
            return;
        }
        InstancePeek::None => {}
    }

    if !is_elevated() {
        require_privileges();
    }

    match acquire_single_instance() {
        InstanceAcquire::AlreadyRunning => {
            if !focus_existing_window() {
                show_user_error("Miao", "Miao 已在运行");
            }
            return;
        }
        InstanceAcquire::Failed => {
            show_user_error("Miao", "无法创建单实例锁");
            return;
        }
        InstanceAcquire::Unique(guard) => {
            // Keep the mutex until process exit so a second launch can see it.
            std::mem::forget(guard);
        }
    }

    if let Err(err) = run_app() {
        show_user_error(
            "Miao",
            &format!(
                "Miao 启动失败：{err}\n日志：{}",
                default_log_path().display()
            ),
        );
        std::process::exit(1);
    }
}

fn run_app() -> Result<(), Box<dyn std::error::Error>> {
    let start_minimized = std::env::args().any(|arg| arg == MINIMIZED_ARG);
    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = tauri::async_runtime::block_on(spawn_server(RuntimeOptions {
                open_browser: false,
                install_tracing: true,
                // 单实例由 mutex 保证，面板端口被占用时让操作系统分配新端口
                port_fallback: true,
                ..RuntimeOptions::default()
            }))?;

            let url = handle.url().to_string();
            if let Err(err) = finish_desktop_shell(app, &url, start_minimized) {
                tauri::async_runtime::block_on(handle.shutdown());
                return Err(err);
            }

            app.manage(Panel(Mutex::new(Some(handle))));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())?;

    app.run(|app, event| {
        if matches!(event, RunEvent::Exit) {
            shutdown_panel(app);
        }
    });
    Ok(())
}

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let log = MenuItem::with_id(app, "log", "打开日志", true, None::<&str>)?;
    let autostart = CheckMenuItemBuilder::with_id("autostart", "开机自启")
        .checked(autostart_is_enabled())
        .build(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &log, &autostart, &quit])?;

    let autostart_for_event = autostart.clone();
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Miao")
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "log" => open_log_file(),
            "autostart" => toggle_autostart(&autostart_for_event),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_main_window(tray.app_handle()),
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => toggle_main_window(tray.app_handle()),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

fn finish_desktop_shell(
    app: &mut tauri::App,
    url: &str,
    start_minimized: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
        .title("Miao")
        .resizable(false)
        .maximized(true)
        .visible(!start_minimized)
        .build()?;
    install_tray(app.handle())?;
    Ok(())
}

/// 托盘勾选开机自启：尝试切换后把勾选状态对齐到真实状态（任务存在性）。
fn toggle_autostart(item: &CheckMenuItem<tauri::Wry>) {
    let target = !autostart_is_enabled();
    if let Err(err) = autostart_set_enabled(target) {
        show_user_error("Miao", &format!("设置开机自启失败：{err}"));
    }
    let _ = item.set_checked(autostart_is_enabled());
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.maximize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 双击托盘图标：窗口显示中（且未最小化）则收回托盘，否则唤出。
/// 与单击「只显示」配合后，双击在两种起始状态下的结果都符合直觉。
fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        let minimized = window.is_minimized().unwrap_or(false);
        if visible && !minimized {
            let _ = window.hide();
        } else {
            show_main_window(app);
        }
    }
}

fn open_log_file() {
    let path = default_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if !path.exists() {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer.exe")
            .raw_arg(explorer_select_arg(&path))
            .spawn();
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn explorer_select_arg(path: &std::path::Path) -> String {
    format!("/select,\"{}\"", path.display())
}

fn shutdown_panel(app: &AppHandle) {
    if let Some(panel) = app.try_state::<Panel>() {
        if let Ok(mut guard) = panel.0.lock() {
            if let Some(handle) = guard.take() {
                tauri::async_runtime::block_on(handle.shutdown());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::explorer_select_arg;
    use std::path::Path;

    #[test]
    fn explorer_select_quotes_paths_with_spaces() {
        let path = Path::new(r"C:\Users\Jane Doe\AppData\Local\io.github.yuxiangluo.miao\miao.log");
        assert_eq!(
            explorer_select_arg(path),
            r#"/select,"C:\Users\Jane Doe\AppData\Local\io.github.yuxiangluo.miao\miao.log""#
        );
    }
}
