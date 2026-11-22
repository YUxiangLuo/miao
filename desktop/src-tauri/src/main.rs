#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use harbor_core::{
    acquire_single_instance, autostart_is_enabled, autostart_repair_if_stale,
    autostart_set_enabled, config_path_from_args, default_log_path, double_click_interval,
    focus_existing_window, is_elevated, peek_single_instance, require_privileges, show_user_error,
    spawn_server, InstanceAcquire, InstancePeek, RuntimeOptions, ServerHandle, MINIMIZED_ARG,
    WINDOW_TITLE,
};
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const DEFAULT_WINDOW_WIDTH: f64 = 1280.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 840.0;
const MIN_WINDOW_WIDTH: f64 = 960.0;
const MIN_WINDOW_HEIGHT: f64 = 640.0;

struct Panel(Mutex<Option<ServerHandle>>);

/// `--minimized` 启动时窗口必须以非最大化状态隐藏创建，避免 tao/Win32 在隐藏窗口上
/// 直接写入 `WS_MAXIMIZE` 污染 `ptMaxPosition`；等首次显式唤出可见后再执行一次最大化。
struct WindowLaunchState {
    pending_initial_maximize: AtomicBool,
}

impl WindowLaunchState {
    fn new(start_minimized: bool) -> Self {
        Self {
            pending_initial_maximize: AtomicBool::new(start_minimized),
        }
    }

    fn take_pending_initial_maximize(&self) -> bool {
        self.pending_initial_maximize.swap(false, Ordering::AcqRel)
    }
}

fn initial_window_maximized(start_minimized: bool) -> bool {
    !start_minimized
}

fn consume_pending_initial_maximize(app: &AppHandle) -> bool {
    app.try_state::<WindowLaunchState>()
        .is_some_and(|state| state.take_pending_initial_maximize())
}

/// 后到实例聚焦先到者的窗口。先到者的窗口在提权后的 setup 阶段才创建，
/// 快速双击时后到者拿到「已在运行」信号可能早于窗口创建——有限轮询等它
/// 出现（最长约 10s），避免误报。
fn focus_existing_window_or_error() {
    for _ in 0..20 {
        if focus_existing_window() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    show_user_error("Harbor", "Harbor 已在运行");
}

fn main() {
    let config_path = match config_path_from_args(
        std::env::args_os()
            .skip(1)
            .filter(|arg| arg != MINIMIZED_ARG),
    ) {
        Ok(path) => path,
        Err(error) => {
            show_user_error("Harbor", &error.to_string());
            return;
        }
    };
    match peek_single_instance() {
        InstancePeek::AlreadyRunning => {
            focus_existing_window_or_error();
            return;
        }
        InstancePeek::Failed => {
            show_user_error("Harbor", "无法检查是否已在运行");
            return;
        }
        InstancePeek::None => {}
    }

    if !is_elevated() {
        require_privileges();
    }

    // Keep the guard in scope until process exit. On Windows its Drop closes
    // the mutex handle; on other platforms it is a zero-sized no-op guard.
    let _instance_guard = match acquire_single_instance() {
        InstanceAcquire::AlreadyRunning => {
            focus_existing_window_or_error();
            return;
        }
        InstanceAcquire::Failed => {
            show_user_error("Harbor", "无法创建单实例锁");
            return;
        }
        InstanceAcquire::Unique(guard) => guard,
    };

    // 自启任务若指向旧 exe（升级/迁移残留），用当前路径重注册
    autostart_repair_if_stale();

    if let Err(err) = run_app(config_path) {
        show_user_error(
            "Harbor",
            &format!(
                "Harbor 启动失败：{err}\n日志：{}",
                default_log_path().display()
            ),
        );
        std::process::exit(1);
    }
}

fn run_app(config_path: Option<std::path::PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let start_minimized = std::env::args_os().any(|arg| arg == MINIMIZED_ARG);
    let app = tauri::Builder::default()
        .setup(move |app| {
            let handle = tauri::async_runtime::block_on(spawn_server(RuntimeOptions {
                open_browser: false,
                install_tracing: true,
                // 单实例由 mutex 保证，面板端口被占用时让操作系统分配新端口
                port_fallback: true,
                config_path,
                ..RuntimeOptions::default()
            }))?;

            let url = handle.url().to_string();
            app.manage(WindowLaunchState::new(start_minimized));
            if let Err(err) = finish_desktop_shell(app, &url, start_minimized) {
                tauri::async_runtime::block_on(handle.shutdown());
                return Err(err);
            }

            app.manage(Panel(Mutex::new(Some(handle))));
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                let _ = window.hide();
                api.prevent_close();
            }
            WindowEvent::Focused(true) => {
                // 后到实例通过 Win32 ShowWindow 唤出托盘隐藏窗口时，tao 内部缓存的
                // WindowFlags::VISIBLE 仍可能停留在 false；同步调用 show() 在已可见时
                // 是纯空操作，同时可为 `--minimized` 首次唤出补做可见态最大化。
                if window.is_visible().unwrap_or(false) {
                    let _ = window.show();
                    if consume_pending_initial_maximize(window.app_handle())
                        && !window.is_maximized().unwrap_or(false)
                    {
                        let _ = window.maximize();
                    }
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())?;

    app.run(|app, event| {
        if matches!(event, RunEvent::Exit) {
            shutdown_panel(app);
        }
    });
    Ok(())
}

/// 托盘单击/双击判定状态。Windows 双击消息序列是 Down,Up,DoubleClick,Up——
/// 若单击立即响应，双击会表现成「先显示、再隐藏、又被尾声 Up 重新显示」。
/// 因此单击延迟一个系统双击时长执行（期间出现双击则作废），
/// 双击后的那次尾声 Up 直接忽略。
#[derive(Default)]
struct TrayClickState {
    generation: AtomicU64,
    last_double_click: Mutex<Option<Instant>>,
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
    let click_state = Arc::new(TrayClickState::default());
    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(WINDOW_TITLE)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "log" => open_log_file(),
            "autostart" => toggle_autostart(&autostart_for_event),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                // 双击尾声的 Up：忽略
                let trailing = click_state
                    .last_double_click
                    .lock()
                    .ok()
                    .and_then(|guard| *guard)
                    .is_some_and(|at| at.elapsed() <= double_click_interval());
                if trailing {
                    return;
                }
                let generation = click_state.generation.fetch_add(1, Ordering::Relaxed) + 1;
                let app = tray.app_handle().clone();
                let state = click_state.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(double_click_interval());
                    if state.generation.load(Ordering::Relaxed) == generation {
                        show_main_window(&app);
                    }
                });
            }
            TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                // 作废挂起的单击，记录时间以吞掉随后的尾声 Up
                click_state.generation.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut guard) = click_state.last_double_click.lock() {
                    *guard = Some(Instant::now());
                }
                toggle_main_window(tray.app_handle());
            }
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
    // 必须保留可缩放边框（WS_SIZEBOX）且禁止在隐藏状态同时设置 maximized(true)：
    // tao 在窗口隐藏时调用 set_maximized(true) 会跳过 ShowWindow(SW_MAXIMIZE)，
    // 直接 SetWindowLongW 写入 WS_MAXIMIZE，把 Win32 内部 ptMaxPosition 污染为
    // 窗口初始坐标；缺少 WS_SIZEBOX 时 Windows 永远不会重置该锚点，导致后续每次
    // 最大化都从非 (0, 0) 开始并超出屏幕/压过任务栏。
    WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
        .title(WINDOW_TITLE)
        .inner_size(DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT)
        .min_inner_size(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
        .center()
        .resizable(true)
        .maximized(initial_window_maximized(start_minimized))
        .visible(!start_minimized)
        .on_new_window(|target, _features| {
            if is_allowed_external_release_url(&target) {
                open_external_url(target.as_str());
            }
            NewWindowResponse::Deny
        })
        .build()?;
    install_tray(app.handle())?;
    Ok(())
}

fn is_allowed_external_release_url(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.path().starts_with("/YUxiangLuo/miao/releases")
}

#[cfg(windows)]
fn open_external_url(url: &str) {
    // Delegate via explorer.exe so the elevated desktop process hands the URL
    // to the interactive medium-IL shell and opens the system default browser.
    let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
}

#[cfg(not(windows))]
fn open_external_url(_url: &str) {}

/// 托盘勾选开机自启：尝试切换后把勾选状态对齐到真实状态（任务存在性）。
fn toggle_autostart(item: &CheckMenuItem<tauri::Wry>) {
    let target = !autostart_is_enabled();
    if let Err(err) = autostart_set_enabled(target) {
        show_user_error("Harbor", &format!("设置开机自启失败：{err}"));
    }
    let _ = item.set_checked(autostart_is_enabled());
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // 先确保窗口可见、取消最小化，再按需执行首次最大化，绝不在隐藏态调用 maximize()。
        let _ = window.show();
        let _ = window.unminimize();
        if consume_pending_initial_maximize(app) && !window.is_maximized().unwrap_or(false) {
            let _ = window.maximize();
        }
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
    use super::{
        explorer_select_arg, initial_window_maximized, is_allowed_external_release_url,
        WindowLaunchState,
    };
    use std::path::Path;

    #[test]
    fn initial_maximize_is_deferred_when_starting_minimized() {
        assert!(initial_window_maximized(false));
        assert!(!initial_window_maximized(true));

        let normal = WindowLaunchState::new(false);
        assert!(!normal.take_pending_initial_maximize());

        let minimized = WindowLaunchState::new(true);
        assert!(minimized.take_pending_initial_maximize());
        assert!(!minimized.take_pending_initial_maximize());
    }

    #[test]
    fn explorer_select_quotes_paths_with_spaces() {
        let path =
            Path::new(r"C:\Users\Jane Doe\AppData\Local\io.github.yuxiangluo.harbor\harbor.log");
        assert_eq!(
            explorer_select_arg(path),
            r#"/select,"C:\Users\Jane Doe\AppData\Local\io.github.yuxiangluo.harbor\harbor.log""#
        );
    }

    #[test]
    fn external_release_url_allowlist_accepts_only_repo_release_https_urls() {
        for allowed in [
            "https://github.com/YUxiangLuo/miao/releases",
            "https://github.com/YUxiangLuo/miao/releases/latest",
            "https://github.com/YUxiangLuo/miao/releases/tag/v0.51.0",
        ] {
            let parsed = tauri::Url::parse(allowed).unwrap();
            assert!(
                is_allowed_external_release_url(&parsed),
                "should allow {allowed}"
            );
        }
        for denied in [
            "http://github.com/YUxiangLuo/miao/releases/latest",
            "https://evil.com/YUxiangLuo/miao/releases/latest",
            "https://github.com/other/repo/releases/latest",
            "file:///C:/Windows/System32/cmd.exe",
            "javascript:alert(1)",
        ] {
            let parsed = tauri::Url::parse(denied).unwrap();
            assert!(
                !is_allowed_external_release_url(&parsed),
                "should deny {denied}"
            );
        }
    }
}
