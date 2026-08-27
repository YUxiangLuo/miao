use std::path::PathBuf;
use std::sync::{atomic::Ordering, Arc};
use std::{fs, io};

use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, RuntimePhase, DEFAULT_PORT};
use crate::services::{
    config::{
        cache_compatibility, fetch_sub_nodes_if_current, gen_config_from_nodes, has_config_cache,
        install_prepared_runtime, load_volatile_config_at, mark_legacy_cache_used,
        persist_effective_node_select, read_sub_nodes_snapshot, refresh_subscriptions,
        restore_config_from_cache, runtime_config_matches_node_select, save_config_cache,
        CacheCompatibility, GenConfigOutcome, RefreshEffect, RefreshPolicy, SubFetchRetry,
        SubSource, ALL_SUBS_FAILED_KEEP_CACHE, ALL_SUBS_FAILED_RETRY, DATA_PLANE_RETRYING,
        REFRESH_FAILED_KEEP_CACHE, REFRESH_VALIDATION_FAILED, REGION_FALLBACK,
        STARTUP_VALIDATION_RETRY, SUBS_REFRESHING_MANUAL,
    },
    proxy::spawn_restore_last_proxy,
    singbox::{
        extract_sing_box_to, is_sing_box_running, start_sing_internal, stop_sing_internal,
        validate_sing_box_config,
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
    /// Override the volatile-layer file (node_select/route_mode). `None` uses the
    /// platform default. Tests must point this at a temp path so they never read
    /// or write the real runtime dir (`/tmp/miao-sing-box`).
    pub volatile_path: Option<PathBuf>,
    /// Tests can skip extracting the embedded kernel when they will not start it.
    pub skip_extract: bool,
    /// Override all generated sing-box artifacts, including the embedded
    /// kernel. This makes config transactions fully hermetic in integration
    /// tests and supports alternate runtime locations without global state.
    pub runtime_dir: Option<PathBuf>,
    /// Override the log file. Windows defaults to
    /// `%LOCALAPPDATA%\io.github.yuxiangluo.miao\miao.log`.
    pub log_path: Option<PathBuf>,
}

/// Running panel. Dropping it requests shutdown; call [`ServerHandle::shutdown`]
/// to wait until axum and sing-box have actually stopped.
pub struct ServerHandle {
    port: u16,
    url: String,
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

    crate::services::version::reconcile_pending_upgrade()?;

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

    let stable_config: crate::models::StableConfig =
        match tokio::fs::read_to_string(&config_path).await {
            Ok(content) => yaml_serde::from_str(&content)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                info!(
                    config_path = ?config_path,
                    "No config file found, using in-memory default configuration"
                );
                crate::models::StableConfig::default()
            }
            Err(e) => return Err(e.into()),
        };
    let runtime_dir = options
        .runtime_dir
        .clone()
        .unwrap_or_else(crate::services::singbox::get_sing_box_home);
    let volatile_path = options.volatile_path.clone().unwrap_or_else(|| {
        if cfg!(windows) {
            crate::services::config::volatile_config_path()
        } else {
            runtime_dir.join("volatile.yaml")
        }
    });
    // 易变层 overlay：node_select/route_mode 的运行值覆盖 config.yaml 解析结果；
    // volatile 文件缺失/损坏时保留 config.yaml 里的同名字段（旧版配置兼容）
    let config = stable_config.effective(load_volatile_config_at(&volatile_path).await);
    let requested_port = options.bind_port.or(config.port).unwrap_or(DEFAULT_PORT);
    let subs_count = config.subs.len();
    let nodes_count = config.nodes.len();

    info!(
        port = requested_port,
        subs = subs_count,
        nodes = nodes_count,
        "Configuration loaded"
    );

    let runtime_paths = {
        let paths = crate::paths::RuntimePaths::new(runtime_dir, &config_path);
        if options.runtime_dir.is_some() {
            paths
        } else {
            paths.with_last_proxy(crate::services::proxy::platform_last_proxy_path())
        }
    };
    let app_state = Arc::new(
        AppState::with_config_layers(
            stable_config,
            config.clone(),
            config_path,
            volatile_path,
            runtime_paths,
        )
        .map_err(|e| AppError::context("Failed to create HTTP client", e))?,
    );
    let state_for_init = app_state.clone();
    let extract_runtime = !options.skip_extract;

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
            _ = initialize_runtime(config, state_for_init, extract_runtime) => {}
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
        init_cancel: Some(init_cancel),
        init_task: Some(init_task),
        shutdown_tx: Some(shutdown_tx),
        server_task: Some(server_task),
    })
}

mod startup;

use startup::initialize_runtime;
#[cfg(test)]
use startup::{
    initialize_runtime_locked, prepare_compatible_startup_cache, recover_data_plane_once,
    refresh_subscriptions_in_background, retry_failed_startup,
};

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

#[cfg(test)]
mod tests;
