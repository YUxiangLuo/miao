use std::path::PathBuf;
use std::sync::Arc;
use std::{fs, io};

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, DEFAULT_PORT};
use crate::services::{
    config::{
        gen_config, has_config_cache, load_volatile_config, persist_effective_node_select,
        record_fresh_snapshot, refresh_subscriptions, restore_config_from_cache,
        runtime_config_matches_node_select, save_config_cache, GenConfigOutcome, RefreshEffect,
        RefreshPolicy, SubFetchRetry, SubSource,
    },
    proxy::restore_last_proxy,
    singbox::{
        extract_sing_box, start_sing_internal, stop_sing_internal, validate_sing_box_config,
    },
};
use crate::state::AppState;
use crate::VERSION;

/// How the panel process should start. The CLI opens a browser; the desktop
/// shell supplies its own window and skips that.
#[derive(Clone, Debug, Default)]
pub struct RuntimeOptions {
    pub open_browser: bool,
    pub install_tracing: bool,
    /// Override the config `port`. `Some(0)` asks the OS for an ephemeral port.
    pub bind_port: Option<u16>,
    /// When the requested panel port is occupied, bind an ephemeral port
    /// instead of failing. The desktop shell sets this: its single-instance
    /// lock is the mutex, so the port is free to move.
    pub port_fallback: bool,
    /// Skip path resolution and load this file (missing file → in-memory default).
    pub config_path: Option<PathBuf>,
    /// Tests can skip extracting the embedded kernel when they will not start it.
    pub skip_extract: bool,
    /// Override the log file. Windows defaults to
    /// `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`.
    pub log_path: Option<PathBuf>,
}

/// Running panel. Dropping it requests shutdown; call [`ServerHandle::shutdown`]
/// to wait until axum and sing-box have actually stopped.
pub struct ServerHandle {
    port: u16,
    url: String,
    log_path: Option<PathBuf>,
    init_cancel: Option<oneshot::Sender<()>>,
    init_task: Option<JoinHandle<()>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_task: Option<JoinHandle<AppResult<()>>>,
}

impl ServerHandle {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn log_path(&self) -> Option<&std::path::Path> {
        self.log_path.as_deref()
    }

    pub async fn shutdown(mut self) {
        request_shutdown(&mut self).await;
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.init_cancel.take() {
            let _ = tx.send(());
        }
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

pub async fn run() -> AppResult<()> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("miao v{}", VERSION);
        return Ok(());
    }

    crate::require_privileges();

    let handle = spawn_server(RuntimeOptions {
        open_browser: true,
        install_tracing: true,
        ..RuntimeOptions::default()
    })
    .await?;

    wait_os_shutdown().await;
    handle.shutdown().await;
    Ok(())
}

pub async fn spawn_server(options: RuntimeOptions) -> AppResult<ServerHandle> {
    let log_path = resolve_log_path(&options);
    if let Some(path) = log_path.as_deref() {
        rotate_oversized_log(path);
    }
    if options.install_tracing {
        install_tracing(log_path.as_deref());
    }
    if let Some(path) = log_path.clone() {
        let _ = crate::paths::set_active_log_path(path);
    }

    #[cfg(windows)]
    crate::services::singbox::ensure_hidden_console();

    if let Ok(current_exe) = std::env::current_exe() {
        let backup_path = format!("{}.bak", current_exe.display());
        if std::path::Path::new(&backup_path).exists() {
            let _ = fs::remove_file(&backup_path);
        }
    }

    info!("Reading configuration...");
    let config_path = match options.config_path.clone() {
        Some(path) => {
            info!(config_path = ?path, source = "explicit", "Resolved configuration path");
            path
        }
        None => {
            let resolution = crate::paths::resolve_config_path()?;
            info!(
                config_path = ?resolution.path,
                source = ?resolution.source,
                "Resolved configuration path"
            );
            resolution.path
        }
    };

    let config: Config = match tokio::fs::read_to_string(&config_path).await {
        Ok(content) => {
            let route_mode_declared = config_declares_route_mode(&content);
            let mut config: Config = serde_yaml::from_str(&content)?;
            if route_mode_declared {
                info!(
                    config_path = ?config_path,
                    "Ignoring route_mode from configuration file; route mode is session-only"
                );
                config.route_mode = Default::default();
            }
            config
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            info!(
                config_path = ?config_path,
                "No config file found, using in-memory default configuration"
            );
            Config::default()
        }
        Err(e) => return Err(e.into()),
    };
    // 易变层 overlay：node_select/route_mode 的运行值覆盖 config.yaml 解析结果；
    // volatile 文件缺失/损坏时保留 config.yaml 里的同名字段（旧版配置兼容）
    let config = config.overlay(load_volatile_config().await);
    let requested_port = options.bind_port.or(config.port).unwrap_or(DEFAULT_PORT);
    let subs_count = config.subs.len();
    let nodes_count = config.nodes.len();

    info!(
        port = requested_port,
        subs = subs_count,
        nodes = nodes_count,
        "Configuration loaded"
    );

    if !options.skip_extract {
        let _ = extract_sing_box()?;
    }

    let app_state = Arc::new(
        AppState::with_config_path(
            config.clone(),
            config_path,
            crate::services::config::volatile_config_path(),
        )
        .map_err(|e| AppError::context("Failed to create HTTP client", e))?,
    );
    let state_for_init = app_state.clone();

    let app = crate::router::build_router(app_state.clone());
    let bind_addr = panel_bind_addr(requested_port);
    let listener = match tokio::net::TcpListener::bind(&bind_addr).await {
        Ok(listener) => listener,
        Err(err) if options.port_fallback && err.kind() == io::ErrorKind::AddrInUse => {
            warn!(
                port = requested_port,
                "Panel port is already in use, falling back to an ephemeral port"
            );
            tokio::net::TcpListener::bind(panel_bind_addr(0)).await?
        }
        Err(err) => return Err(err.into()),
    };
    let port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}");
    info!(port = port, url = %url, "Miao panel started");

    if options.open_browser && config.subs.is_empty() && config.nodes.is_empty() {
        let browser_url = url.clone();
        tokio::spawn(async move {
            open_onboarding_browser(browser_url).await;
        });
    }

    let (init_cancel, init_rx) = oneshot::channel();
    let init_task = tokio::spawn(async move {
        tokio::select! {
            _ = init_rx => {
                info!("Runtime initialization cancelled");
            }
            _ = initialize_runtime(config, state_for_init) => {}
        }
    });

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let state_for_shutdown = app_state.clone();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
                info!("Shutting down, stopping sing-box...");
                stop_sing_internal(&state_for_shutdown).await;
            })
            .await?;
        Ok(())
    });

    Ok(ServerHandle {
        port,
        url,
        log_path,
        init_cancel: Some(init_cancel),
        init_task: Some(init_task),
        shutdown_tx: Some(shutdown_tx),
        server_task: Some(server_task),
    })
}

async fn initialize_runtime(config: Config, state: Arc<AppState>) {
    let _config_update = state.config_update.lock().await;

    if config.subs.is_empty() && config.nodes.is_empty() {
        info!("No subscriptions or nodes configured, waiting for onboarding");
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        return;
    }

    // 快速通道：存在上次成功运行的缓存配置 → 先起内核（秒开），订阅改为后台刷新。
    // 缓存读取/校验/启动任何一步失败都落回同步拉取路径。
    if has_config_cache() {
        let cache_usable = match restore_config_from_cache().await {
            Ok(()) => match validate_sing_box_config().await {
                Ok(()) => true,
                Err(err) => {
                    warn!(error = %err, "Cached config failed validation, fetching subscriptions");
                    false
                }
            },
            Err(err) => {
                warn!(error = %err, "Failed to restore cached config, fetching subscriptions");
                false
            }
        };

        if cache_usable {
            let cache_matches_select = match tokio::fs::read_to_string(
                crate::services::singbox::get_sing_box_home().join("config.json"),
            )
            .await
            {
                Ok(content) => serde_json::from_str(&content).ok().is_some_and(|json| {
                    runtime_config_matches_node_select(&json, config.node_select)
                }),
                Err(_) => false,
            };
            if !cache_matches_select {
                warn!("Cached config does not match node_select; regenerating");
            } else {
                #[cfg(not(windows))]
                {
                    info!("Checking dependencies...");
                    if let Err(e) =
                        crate::services::openwrt::check_and_install_openwrt_dependencies().await
                    {
                        error!("Failed to check or install OpenWrt dependencies: {}", e);
                    }
                }

                match start_sing_internal(&state).await {
                    Ok(()) => {
                        info!("sing-box started from cached config");
                        let state_for_proxy = state.clone();
                        tokio::spawn(async move {
                            restore_last_proxy(&state_for_proxy).await;
                        });
                        state
                            .initializing
                            .store(false, std::sync::atomic::Ordering::Relaxed);

                        // 内核已在跑，初始化结束；订阅刷新在后台进行（仍持 config_update
                        // 锁，与面板的配置变更互斥；随初始化任务一同被关停取消）
                        refresh_subscriptions_in_background(&config, &state).await;
                        return;
                    }
                    Err(err) => {
                        error!(error = %err, "Failed to start sing-box from cache, fetching subscriptions");
                    }
                }
            }
        }
    }

    info!("Generating initial config...");
    let mut all_subs_failed = false;
    let mut fresh_gen: Option<GenConfigOutcome> = None;
    match gen_config(&config, &state, SubFetchRetry::Startup).await {
        Ok(outcome) => {
            if let Err(err) = persist_effective_node_select(&state, outcome.node_select).await {
                warn!(error = %err, "Failed to persist effective node_select after generate");
            }
            if !config.node_select.is_manual() && outcome.node_select.is_manual() {
                *state.config_warning.lock().await =
                    Some("该地区没有可用节点，已切回手动选择".to_string());
            }
            if !outcome.has_sub_nodes && !config.subs.is_empty() {
                all_subs_failed = true;
            }
            fresh_gen = Some(outcome);
        }
        Err(e) => {
            error!(error = %e, "Failed to generate config");
            match restore_config_from_cache().await {
                Ok(_) => {
                    warn!("Using cached config as fallback");
                    all_subs_failed = true;
                }
                Err(cache_err) => {
                    error!(error = %cache_err, "No cached config available");
                    *state.config_warning.lock().await =
                        Some("所有订阅获取失败且无可用缓存，请添加订阅或手动节点".to_string());
                    state
                        .initializing
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    return;
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        info!("Checking dependencies...");
        if let Err(e) = crate::services::openwrt::check_and_install_openwrt_dependencies().await {
            error!("Failed to check or install OpenWrt dependencies: {}", e);
        }
    }

    match start_sing_internal(&state).await {
        Ok(_) => {
            info!("sing-box started successfully");
            save_config_cache().await;
            // 启动成功等价于配置可用：把本次拉取的节点集落成快照，供本地语义变更零网络重建
            if let Some(outcome) = &fresh_gen {
                record_fresh_snapshot(&config, outcome).await;
            }
            if all_subs_failed && state.config_warning.lock().await.is_none() {
                warn!("所有订阅获取失败，请检查当前订阅");
                *state.config_warning.lock().await =
                    Some("所有订阅获取失败，请检查当前订阅".to_string());
            }
            let state_for_proxy = state.clone();
            tokio::spawn(async move {
                restore_last_proxy(&state_for_proxy).await;
            });
        }
        Err(e) => error!("Failed to start sing-box: {}", e),
    }
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
}

/// 后台订阅刷新（快速通道启动后调用，调用方持有 config_update 锁）：
/// 机制全部收敛在 services::config::refresh_subscriptions；这里只按 outcome 决定告警与收尾。
async fn refresh_subscriptions_in_background(config: &Config, state: &Arc<AppState>) {
    match refresh_subscriptions(config, state, RefreshPolicy::Startup, SubSource::Fetch).await {
        Ok(outcome) => match outcome.effect {
            RefreshEffect::Restarted => {
                info!("sing-box restarted with refreshed subscriptions");
                save_config_cache().await;
                if !config.node_select.is_manual() && outcome.node_select.is_manual() {
                    *state.config_warning.lock().await =
                        Some("该地区没有可用节点，已切回手动选择".to_string());
                }
                let state_for_proxy = state.clone();
                tokio::spawn(async move {
                    restore_last_proxy(&state_for_proxy).await;
                });
            }
            RefreshEffect::SkippedUnchanged => {
                if !config.node_select.is_manual() && outcome.node_select.is_manual() {
                    *state.config_warning.lock().await =
                        Some("该地区没有可用节点，已切回手动选择".to_string());
                }
            }
            RefreshEffect::KeptRunningOnTotalFailure => {
                warn!("所有订阅获取失败，继续使用缓存配置运行");
                *state.config_warning.lock().await =
                    Some("所有订阅获取失败，继续使用缓存配置运行".to_string());
            }
            RefreshEffect::KeptRunningOnValidationFailure => {
                error!("Refreshed config failed validation; keeping cached config");
                *state.config_warning.lock().await =
                    Some("订阅刷新后的配置校验失败，继续使用缓存配置运行".to_string());
            }
        },
        Err(err) => {
            warn!(error = %err, "Background subscription refresh failed");
            *state.config_warning.lock().await =
                Some("订阅刷新失败，继续使用缓存配置运行".to_string());
        }
    }
}

async fn request_shutdown(handle: &mut ServerHandle) {
    if let Some(tx) = handle.init_cancel.take() {
        let _ = tx.send(());
    }
    if let Some(mut task) = handle.init_task.take() {
        match tokio::time::timeout(Duration::from_secs(8), &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) if err.is_cancelled() => {}
            Ok(Err(err)) => {
                error!(error = %err, "Runtime initialization task failed on shutdown")
            }
            Err(_) => {
                warn!("Runtime initialization did not finish before shutdown");
                task.abort();
                let _ = task.await;
            }
        }
    }
    if let Some(tx) = handle.shutdown_tx.take() {
        let _ = tx.send(());
    }
    if let Some(task) = handle.server_task.take() {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => error!(error = %err, "Panel server returned error on shutdown"),
            Err(err) => error!(error = %err, "Panel server task failed on shutdown"),
        }
    }
}

async fn wait_os_shutdown() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.expect("failed to install Ctrl+C handler");
            }
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    }
}

fn resolve_log_path(options: &RuntimeOptions) -> Option<PathBuf> {
    if let Some(path) = &options.log_path {
        return Some(path.clone());
    }
    if cfg!(windows) {
        Some(crate::paths::default_log_path())
    } else {
        None
    }
}

/// 日志文件（tracing + sing-box 输出）只追加不清理，长期常驻会无限增长。
/// 启动时把超限的日志改名为 `.old`（覆盖上一份），实现单份滚动。
const MAX_LOG_FILE_BYTES: u64 = 8 * 1024 * 1024;

fn rotate_oversized_log(path: &std::path::Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() <= MAX_LOG_FILE_BYTES {
        return;
    }
    let rotated = rotated_log_path(path);
    match fs::rename(path, &rotated) {
        Ok(()) => info!(log = ?path, rotated_to = ?rotated, "Rotated oversized log file"),
        Err(err) => warn!(log = ?path, error = %err, "Failed to rotate oversized log file"),
    }
}

fn rotated_log_path(path: &std::path::Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".old");
    path.with_file_name(name)
}

fn install_tracing(log_path: Option<&std::path::Path>) {
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    let filter = || {
        tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into())
    };

    if let Some(path) = log_path {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => {
                let writer = std::io::stdout.and(std::sync::Mutex::new(file));
                let _ = tracing_subscriber::fmt()
                    .with_env_filter(filter())
                    .with_writer(writer)
                    .try_init();
                return;
            }
            Err(err) => {
                eprintln!("Failed to open log file {}: {err}", path.display());
            }
        }
    }

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter())
        .try_init();
}

fn panel_bind_addr(port: u16) -> String {
    #[cfg(windows)]
    {
        format!("127.0.0.1:{port}")
    }
    #[cfg(not(windows))]
    {
        format!("0.0.0.0:{port}")
    }
}

fn browser_launch_env() -> Vec<(String, String)> {
    let mut envs = Vec::new();

    for key in ["DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY"] {
        if let Ok(value) = std::env::var(key) {
            envs.push((key.to_string(), value));
        }
    }

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok().or_else(|| {
        std::env::var("SUDO_UID")
            .ok()
            .map(|uid| format!("/run/user/{uid}"))
    });

    if let Some(runtime_dir) = runtime_dir {
        envs.push(("XDG_RUNTIME_DIR".to_string(), runtime_dir.clone()));

        let bus_address = std::env::var("DBUS_SESSION_BUS_ADDRESS")
            .ok()
            .unwrap_or_else(|| format!("unix:path={runtime_dir}/bus"));
        envs.push(("DBUS_SESSION_BUS_ADDRESS".to_string(), bus_address));
    } else if let Ok(bus_address) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        envs.push(("DBUS_SESSION_BUS_ADDRESS".to_string(), bus_address));
    }

    envs
}

async fn open_onboarding_browser(url: String) {
    let has_graphical_session =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !has_graphical_session {
        return;
    }

    let launch_env = browser_launch_env();
    let sudo_user = std::env::var("SUDO_USER")
        .ok()
        .filter(|user| !user.is_empty());
    let use_runuser = sudo_user.is_some();
    let mut command = if let Some(sudo_user) = sudo_user {
        let mut command = tokio::process::Command::new("runuser");
        command.arg("-u").arg(sudo_user).arg("--").arg("env");
        for (key, value) in &launch_env {
            command.arg(format!("{key}={value}"));
        }
        command.arg("xdg-open");
        command
    } else {
        tokio::process::Command::new("xdg-open")
    };

    command.arg(&url);
    if !use_runuser {
        command.envs(launch_env);
    }

    match command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
    {
        Ok(status) if status.success() => {}
        Ok(status) => warn!(
            url = %url,
            status = ?status.code(),
            "Failed to auto-open onboarding URL in browser"
        ),
        Err(err) => warn!(url = %url, error = %err, "Failed to launch browser opener"),
    }
}

fn config_declares_route_mode(content: &str) -> bool {
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(content) else {
        return false;
    };

    value
        .as_mapping()
        .is_some_and(|mapping| mapping.contains_key("route_mode"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{config_declares_route_mode, panel_bind_addr, spawn_server, RuntimeOptions};

    /// Hold a port the panel would bind. Windows needs SO_EXCLUSIVEADDRUSE so
    /// Tokio's SO_REUSEADDR cannot hijack it.
    fn occupy_panel_port() -> (std::net::TcpListener, u16) {
        let probe = std::net::TcpListener::bind(panel_bind_addr(0)).expect("probe port");
        let port = probe.local_addr().expect("probe addr").port();
        drop(probe);

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawSocket;
            use windows_sys::Win32::Networking::WinSock::{
                setsockopt, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
            };

            let socket = tokio::net::TcpSocket::new_v4().expect("tcp socket");
            let enable: i32 = 1;
            let rc = unsafe {
                setsockopt(
                    socket.as_raw_socket() as _,
                    SOL_SOCKET,
                    SO_EXCLUSIVEADDRUSE,
                    (&enable as *const i32).cast(),
                    std::mem::size_of_val(&enable) as i32,
                )
            };
            assert_ne!(rc, -1, "SO_EXCLUSIVEADDRUSE");
            socket
                .bind(panel_bind_addr(port).parse().expect("addr"))
                .expect("exclusive bind");
            let listener = socket
                .listen(1)
                .expect("listen")
                .into_std()
                .expect("into std listener");
            (listener, port)
        }

        #[cfg(not(windows))]
        {
            let listener = std::net::TcpListener::bind(panel_bind_addr(port)).expect("occupy port");
            (listener, port)
        }
    }

    #[test]
    fn config_declares_route_mode_when_top_level_key_exists() {
        let yaml = r#"
port: 6161
route_mode: global
subs: []
"#;

        assert!(config_declares_route_mode(yaml));
    }

    #[test]
    fn config_declares_route_mode_ignores_nested_key() {
        let yaml = r#"
custom_rules:
  - '{"route_mode":"global"}'
"#;

        assert!(!config_declares_route_mode(yaml));
    }

    #[test]
    fn config_declares_route_mode_handles_invalid_yaml() {
        assert!(!config_declares_route_mode("route_mode: ["));
    }

    #[test]
    fn resolve_log_path_honors_explicit_override() {
        let path = PathBuf::from("/tmp/miao-test.log");
        let options = RuntimeOptions {
            log_path: Some(path.clone()),
            ..RuntimeOptions::default()
        };
        assert_eq!(super::resolve_log_path(&options), Some(path));
    }

    #[tokio::test]
    async fn spawn_server_serves_status_and_shuts_down() {
        let config_path = unique_test_config_path();

        let handle = spawn_server(RuntimeOptions {
            open_browser: false,
            install_tracing: false,
            bind_port: Some(0),
            port_fallback: false,
            config_path: Some(config_path),
            skip_extract: true,
            log_path: None,
        })
        .await
        .expect("spawn panel");

        assert!(handle.url().starts_with("http://127.0.0.1:"));
        assert_ne!(handle.port(), 0);

        let client = reqwest::Client::new();
        let status_url = format!("{}/api/status", handle.url());
        let mut last_error = None;
        let mut body = None;
        for _ in 0..20 {
            match client.get(&status_url).send().await {
                Ok(response) => {
                    assert!(response.status().is_success());
                    body = Some(response.text().await.expect("status body"));
                    last_error = None;
                    break;
                }
                Err(err) => {
                    last_error = Some(err);
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
        if let Some(err) = last_error {
            panic!("panel did not become ready: {err}");
        }
        let body = body.expect("status body");
        assert!(body.contains("stopped") || body.contains("running"));

        let url = handle.url().to_string();
        handle.shutdown().await;

        let after = client.get(format!("{url}/api/status")).send().await;
        assert!(
            after.is_err(),
            "panel should reject requests after shutdown"
        );
    }

    #[tokio::test]
    async fn spawn_server_falls_back_to_ephemeral_port_when_occupied() {
        let (blocker, occupied_port) = occupy_panel_port();

        let handle = spawn_server(RuntimeOptions {
            open_browser: false,
            install_tracing: false,
            bind_port: Some(occupied_port),
            port_fallback: true,
            config_path: Some(unique_test_config_path()),
            skip_extract: true,
            log_path: None,
        })
        .await
        .expect("spawn panel with port fallback");

        assert_ne!(handle.port(), occupied_port);
        handle.shutdown().await;
        drop(blocker);
    }

    #[tokio::test]
    async fn spawn_server_without_port_fallback_fails_when_occupied() {
        let (blocker, occupied_port) = occupy_panel_port();

        let result = spawn_server(RuntimeOptions {
            open_browser: false,
            install_tracing: false,
            bind_port: Some(occupied_port),
            port_fallback: false,
            config_path: Some(unique_test_config_path()),
            skip_extract: true,
            log_path: None,
        })
        .await;

        assert!(result.is_err());
        drop(blocker);
    }

    #[test]
    fn rotated_log_path_appends_old_suffix() {
        let path = PathBuf::from("/tmp/miao.log");
        assert_eq!(
            super::rotated_log_path(&path),
            PathBuf::from("/tmp/miao.log.old")
        );
    }

    #[test]
    fn rotate_oversized_log_keeps_small_file() {
        let path = unique_test_log_path("small");
        std::fs::write(&path, b"small").expect("write small log");

        super::rotate_oversized_log(&path);

        assert!(path.exists());
        assert!(!super::rotated_log_path(&path).exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rotate_oversized_log_renames_big_file() {
        let path = unique_test_log_path("big");
        let big = vec![b'x'; (super::MAX_LOG_FILE_BYTES + 1) as usize];
        std::fs::write(&path, &big).expect("write big log");

        super::rotate_oversized_log(&path);

        assert!(!path.exists());
        let rotated = super::rotated_log_path(&path);
        assert_eq!(
            std::fs::metadata(&rotated).expect("rotated log").len(),
            super::MAX_LOG_FILE_BYTES + 1
        );
        let _ = std::fs::remove_file(&rotated);
    }

    fn unique_test_config_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "miao-spawn-server-test-{}-{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ))
    }

    fn unique_test_log_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "miao-rotate-{tag}-{}-{}.log",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ))
    }
}
