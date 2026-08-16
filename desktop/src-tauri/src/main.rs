#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;

use miao_core::{require_privileges, spawn_server, RuntimeOptions, ServerHandle};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

struct Panel(Mutex<Option<ServerHandle>>);

fn main() {
    require_privileges();

    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = tauri::async_runtime::block_on(spawn_server(RuntimeOptions {
                open_browser: false,
                install_tracing: true,
                ..RuntimeOptions::default()
            }))?;

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
        .build(tauri::generate_context!())
        .expect("failed to build Miao desktop");

    app.run(|app, event| {
        if matches!(event, RunEvent::Exit) {
            shutdown_panel(app);
        }
    });
}

fn install_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Miao")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
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

fn shutdown_panel(app: &AppHandle) {
    if let Some(panel) = app.try_state::<Panel>() {
        if let Ok(mut guard) = panel.0.lock() {
            if let Some(handle) = guard.take() {
                tauri::async_runtime::block_on(handle.shutdown());
            }
        }
    }
}
