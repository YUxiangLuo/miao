#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use miao_core::{
    acquire_single_instance, default_log_path, focus_existing_window, require_privileges,
    show_user_error, spawn_server, InstanceAcquire, RuntimeOptions, ServerHandle,
};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

struct Panel(Mutex<Option<ServerHandle>>);

fn main() {
    require_privileges();

    match acquire_single_instance() {
        InstanceAcquire::AlreadyRunning => {
            if !focus_existing_window() {
                show_user_error("Miao", "Miao 已在运行");
            }
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
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = match tauri::async_runtime::block_on(spawn_server(RuntimeOptions {
                open_browser: false,
                install_tracing: true,
                ..RuntimeOptions::default()
            })) {
                Ok(handle) => handle,
                Err(err) => {
                    show_user_error(
                        "Miao",
                        &format!(
                            "Miao 启动失败：{err}\n日志：{}",
                            default_log_path().display()
                        ),
                    );
                    return Err(err.into());
                }
            };

            let url = handle.url().to_string();
            app.manage(Panel(Mutex::new(Some(handle))));

            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title("Miao")
                .inner_size(1280.0, 840.0)
                .min_inner_size(960.0, 640.0)
                .build()?;

            install_tray(app.handle())?;
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
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &log, &quit])?;

    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Miao")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "log" => open_log_file(),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
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
        let _ = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
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
