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
        cache_compatibility, fetch_sub_nodes_if_current, gen_config, gen_config_from_nodes,
        has_config_cache, install_prepared_runtime, load_volatile_config_at,
        mark_legacy_cache_used, persist_effective_node_select, read_sub_nodes_snapshot,
        record_fresh_snapshot, refresh_subscriptions, restore_config_from_cache,
        runtime_config_matches_node_select, save_config_cache, CacheCompatibility,
        GenConfigOutcome, RefreshEffect, RefreshPolicy, SubFetchRetry, SubSource,
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

    let runtime_paths = crate::paths::RuntimePaths::new(runtime_dir, &config_path);
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
        log_path,
        init_cancel: Some(init_cancel),
        init_task: Some(init_task),
        shutdown_tx: Some(shutdown_tx),
        server_task: Some(server_task),
    })
}

async fn initialize_runtime(config: Config, state: Arc<AppState>, extract_runtime: bool) {
    if extract_runtime {
        state.set_runtime_phase(RuntimePhase::Extracting);
        let runtime_dir = state.runtime_paths.runtime_dir.clone();
        let extracted =
            tokio::task::spawn_blocking(move || extract_sing_box_to(&runtime_dir)).await;
        match extracted {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                error!(error = %err, "Failed to prepare embedded sing-box runtime");
                *state.config_warning.lock().await = Some(format!("准备 sing-box 内核失败：{err}"));
                state
                    .runtime_ready
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Failed);
                state
                    .initializing
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
            Err(err) => {
                error!(error = %err, "Embedded sing-box extraction task failed");
                *state.config_warning.lock().await =
                    Some(format!("准备 sing-box 内核任务失败：{err}"));
                state
                    .runtime_ready
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Failed);
                state
                    .initializing
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                return;
            }
        }
    }

    state.set_runtime_phase(RuntimePhase::Initializing);
    // config_update 锁只覆盖「起内核」的本地操作；快速通道成功后的订阅后台刷新
    // 移出锁外拉取（见 refresh_subscriptions_in_background），网络退避不再阻塞面板写操作
    let started_from_local_state = {
        let _config_update = state.config_update.lock().await;
        initialize_runtime_locked(&config, &state).await
    };

    if started_from_local_state {
        refresh_subscriptions_in_background(&config, &state).await;
    } else if should_retry_failed_startup(&state).await {
        // The panel itself is healthy and must stay available. Mark the app
        // upgrade healthy, then keep retrying the data plane in this cancellable
        // initialization task instead of relying on systemd to restart us.
        crate::services::version::mark_upgrade_healthy();
        retry_failed_startup(&state).await;
        return;
    }
    crate::services::version::mark_upgrade_healthy();
}

async fn should_retry_failed_startup(state: &Arc<AppState>) -> bool {
    if state.runtime_ready.load(Ordering::Relaxed)
        || !state.service_should_run.load(Ordering::Relaxed)
    {
        return false;
    }
    let config = state.config.read().await;
    !config.subs.is_empty() || !config.nodes.is_empty()
}

/// Check cache provenance and runtime semantics before copying it into the
/// active slot. `Ok(true)` identifies the one-time legacy compatibility path.
async fn prepare_compatible_startup_cache(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let compatibility = cache_compatibility(state, config).await;
    let legacy = match compatibility {
        CacheCompatibility::Verified => false,
        CacheCompatibility::Legacy => true,
        CacheCompatibility::Incompatible(reason) => {
            return Err(AppError::message(format!(
                "Cached config provenance check failed: {reason}"
            )));
        }
    };

    // Validate and inspect the cache in place. A rejected cache must not
    // overwrite config.json, which is also the rollback snapshot source.
    validate_sing_box_config(state, &state.runtime_paths.config_cache).await?;
    let content = tokio::fs::read_to_string(&state.runtime_paths.config_cache)
        .await
        .map_err(|e| AppError::context("Failed to read cached config", e))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::context("Cached config is invalid JSON", e))?;
    if !runtime_config_matches_node_select(&json, config.node_select) {
        return Err(AppError::message(
            "Cached config does not match the effective node_select",
        ));
    }

    restore_config_from_cache(state).await?;
    Ok(legacy)
}

#[cfg(not(windows))]
async fn check_startup_dependencies() {
    info!("Checking dependencies...");
    if let Err(err) = crate::services::openwrt::check_and_install_openwrt_dependencies().await {
        error!(error = %err, "Failed to check or install OpenWrt dependencies");
    }
}

#[cfg(windows)]
async fn check_startup_dependencies() {}

/// Install, validate and start a config rebuilt entirely from local node
/// material. A successful local start becomes the new verified exact cache;
/// subscription fetching can then happen in the background.
async fn start_prepared_local_runtime(
    config: &Config,
    state: &Arc<AppState>,
    outcome: &GenConfigOutcome,
    source: &'static str,
) -> AppResult<()> {
    state.set_runtime_phase(RuntimePhase::Validating);
    install_prepared_runtime(state, outcome).await?;
    if let Err(err) = persist_effective_node_select(state, outcome.node_select).await {
        warn!(error = %err, "Failed to persist effective node_select after local startup rebuild");
    }

    check_startup_dependencies().await;
    start_sing_internal(state).await?;
    info!(source, "sing-box started from local startup material");

    save_config_cache(state).await;
    *state.skipped_rules.lock().await = outcome.skipped_rules.clone();
    *state.config_warning.lock().await =
        if !config.node_select.is_manual() && outcome.node_select.is_manual() {
            Some("该地区没有可用节点，已切回手动选择".to_string())
        } else if !outcome.has_sub_nodes && !config.subs.is_empty() {
            Some("订阅正在后台刷新，暂时使用手动节点".to_string())
        } else {
            None
        };
    spawn_restore_last_proxy(state);
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// 持锁执行的初始化。返回 true = 内核已用本地材料快速启动，订阅需改为
/// 后台刷新；false = 无需后台刷新（无配置/无订阅/已走同步拉取路径）。
async fn initialize_runtime_locked(config: &Config, state: &Arc<AppState>) -> bool {
    if config.subs.is_empty() && config.nodes.is_empty() {
        info!("No subscriptions or nodes configured, waiting for onboarding");
        state
            .runtime_ready
            .store(false, std::sync::atomic::Ordering::Relaxed);
        state.set_runtime_phase(RuntimePhase::Stopped);
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        return false;
    }

    // 快速通道：存在上次成功运行的缓存配置 → 先起内核（秒开），订阅改为后台刷新。
    // 缓存读取/校验/启动任何一步失败都落回同步拉取路径。
    if has_config_cache(state) {
        state.set_runtime_phase(RuntimePhase::Validating);
        match prepare_compatible_startup_cache(config, state).await {
            Ok(legacy_cache) => {
                check_startup_dependencies().await;

                match start_sing_internal(state).await {
                    Ok(()) => {
                        info!("sing-box started from cached config");
                        if legacy_cache {
                            mark_legacy_cache_used(state).await;
                        }
                        spawn_restore_last_proxy(state);
                        state
                            .initializing
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        // A legacy cache still needs one local regeneration to
                        // prove all runtime inputs and upgrade its manifest,
                        // even when there are no subscriptions to fetch.
                        return legacy_cache || !config.subs.is_empty();
                    }
                    Err(err) => {
                        error!(error = %err, "Failed to start sing-box from cache, fetching subscriptions");
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "Cached config is not eligible for startup; regenerating");
            }
        }
    }

    // 本地 tier 2：精确缓存缺失/不兼容时，用上次订阅节点集按当前规则、
    // 路由模式和节点选择重新生成。订阅列表必须逐项一致，避免用错来源。
    if let Some(snapshot) = read_sub_nodes_snapshot(state).await {
        if snapshot.matches_subs(&config.subs) {
            info!("Rebuilding startup config from subscription node snapshot (no network)");
            match gen_config_from_nodes(config, state, snapshot.into_fetched_nodes()).await {
                Ok(outcome) => {
                    match start_prepared_local_runtime(config, state, &outcome, "node_snapshot")
                        .await
                    {
                        Ok(()) => return !config.subs.is_empty(),
                        Err(err) => {
                            warn!(error = %err, "Failed to start from subscription node snapshot")
                        }
                    }
                }
                Err(err) => {
                    warn!(error = %err, "Failed to rebuild from subscription node snapshot")
                }
            }
        } else {
            warn!("Subscription node snapshot does not match current subscription list");
        }
    }

    // 本地 tier 3：即使从未成功拉取过订阅，只要配置中还有有效手动节点，
    // 也先让数据面可用；订阅继续在后台刷新，成功后再无缝更新运行配置。
    if !config.nodes.is_empty() {
        info!("Building startup config from manual nodes (no network)");
        match gen_config_from_nodes(config, state, Vec::new()).await {
            Ok(outcome) => {
                match start_prepared_local_runtime(config, state, &outcome, "manual_nodes").await {
                    Ok(()) => return !config.subs.is_empty(),
                    Err(err) => warn!(error = %err, "Failed to start from manual nodes"),
                }
            }
            Err(err) => warn!(error = %err, "No valid manual-node startup config available"),
        }
    }

    info!("Generating initial config...");
    state.set_runtime_phase(RuntimePhase::FetchingSubscriptions);
    let mut all_subs_failed = false;
    let mut fresh_gen: Option<GenConfigOutcome> = None;
    let mut fallback_cache_legacy: Option<bool> = None;
    match gen_config(config, state, SubFetchRetry::Startup).await {
        Ok(outcome) => match install_prepared_runtime(state, &outcome).await {
            Ok(()) => {
                if let Err(err) = persist_effective_node_select(state, outcome.node_select).await {
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
                error!(error = %e, "Generated startup config failed validation or installation");
                match prepare_compatible_startup_cache(config, state).await {
                    Ok(legacy_cache) => {
                        warn!("Using cached config as fallback");
                        all_subs_failed = true;
                        fallback_cache_legacy = Some(legacy_cache);
                    }
                    Err(cache_err) => {
                        error!(error = %cache_err, "No cached config available");
                        *state.config_warning.lock().await = Some(
                            "生成的配置校验失败且无可用缓存，请检查订阅或手动节点".to_string(),
                        );
                        state
                            .runtime_ready
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        state.set_runtime_phase(RuntimePhase::Failed);
                        state
                            .initializing
                            .store(false, std::sync::atomic::Ordering::Relaxed);
                        return false;
                    }
                }
            }
        },
        Err(e) => {
            error!(error = %e, "Failed to generate config");
            match prepare_compatible_startup_cache(config, state).await {
                Ok(legacy_cache) => {
                    warn!("Using cached config as fallback");
                    all_subs_failed = true;
                    fallback_cache_legacy = Some(legacy_cache);
                }
                Err(cache_err) => {
                    error!(error = %cache_err, "No cached config available");
                    *state.config_warning.lock().await =
                        Some("所有订阅获取失败且无可用缓存，请添加订阅或手动节点".to_string());
                    state
                        .runtime_ready
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    state.set_runtime_phase(RuntimePhase::Failed);
                    state
                        .initializing
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    return false;
                }
            }
        }
    }

    check_startup_dependencies().await;

    match start_sing_internal(state).await {
        Ok(_) => {
            info!("sing-box started successfully");
            match fallback_cache_legacy {
                Some(true) => mark_legacy_cache_used(state).await,
                Some(false) => {
                    // The verified cache and manifest are already current.
                }
                None => save_config_cache(state).await,
            }
            // 启动成功等价于配置可用：把本次拉取的节点集落成快照，供本地语义变更零网络重建
            if let Some(outcome) = &fresh_gen {
                record_fresh_snapshot(config, state, outcome).await;
                *state.skipped_rules.lock().await = outcome.skipped_rules.clone();
            }
            if all_subs_failed && state.config_warning.lock().await.is_none() {
                warn!("所有订阅获取失败，请检查当前订阅");
                *state.config_warning.lock().await =
                    Some("所有订阅获取失败，请检查当前订阅".to_string());
            }
            spawn_restore_last_proxy(state);
        }
        Err(e) => {
            state
                .runtime_ready
                .store(false, std::sync::atomic::Ordering::Relaxed);
            state.set_runtime_phase(RuntimePhase::Failed);
            error!("Failed to start sing-box: {}", e);
        }
    }
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    false
}

/// 后台订阅刷新（快速通道启动后调用）。
/// 阶段 1 不持锁拉取订阅节点集：绝对预算 20s，不阻塞面板写操作；
/// 阶段 2 持锁落地：拉取期间订阅列表被改过（面板编辑已按新配置自行应用）
/// 或服务被显式停止，则放弃本次刷新。
/// 机制全部收敛在 services::config::refresh_subscriptions；这里只按 outcome 决定告警与收尾。
async fn refresh_subscriptions_in_background(config: &Config, state: &Arc<AppState>) {
    let refresh_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    let background_phase = if config.subs.is_empty() {
        RuntimePhase::Validating
    } else {
        RuntimePhase::RefreshingSubscriptions
    };
    state.set_runtime_phase(background_phase);
    let nodes =
        fetch_sub_nodes_if_current(config, state, SubFetchRetry::Startup, refresh_generation).await;

    let _config_update = state.config_update.lock().await;
    if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
        info!("Startup subscription refresh was superseded by a foreground refresh; skipping");
        return;
    }
    let current = state.config.read().await.clone();
    if current.subs != config.subs {
        info!(
            "Subscriptions changed during background refresh; skipping (panel edit already applied)"
        );
        return;
    }
    if !state
        .service_should_run
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        info!("Service stopped during background refresh; skipping");
        return;
    }

    match refresh_subscriptions(
        &current,
        state,
        RefreshPolicy::Startup,
        SubSource::Prefetched(nodes),
    )
    .await
    {
        Ok(outcome) => match outcome.effect {
            RefreshEffect::Activated => {
                info!("sing-box activated refreshed subscriptions");
                save_config_cache(state).await;
                *state.config_warning.lock().await =
                    if !config.node_select.is_manual() && outcome.node_select.is_manual() {
                        Some("该地区没有可用节点，已切回手动选择".to_string())
                    } else {
                        None
                    };
                spawn_restore_last_proxy(state);
            }
            RefreshEffect::SkippedUnchanged => {
                // This also upgrades a one-time legacy cache marker to a
                // verified manifest after the freshly generated bytes match
                // the validated running cache.
                save_config_cache(state).await;
                *state.config_warning.lock().await =
                    if !config.node_select.is_manual() && outcome.node_select.is_manual() {
                        Some("该地区没有可用节点，已切回手动选择".to_string())
                    } else {
                        None
                    };
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
    if state.runtime_phase() == background_phase {
        state.set_runtime_phase(RuntimePhase::Ready);
    }
}

#[cfg(not(test))]
const STARTUP_RECOVERY_INITIAL_DELAY: Duration = Duration::from_secs(5);
#[cfg(not(test))]
const STARTUP_RECOVERY_MAX_DELAY: Duration = Duration::from_secs(60);
#[cfg(test)]
const STARTUP_RECOVERY_INITIAL_DELAY: Duration = Duration::from_millis(20);
#[cfg(test)]
const STARTUP_RECOVERY_MAX_DELAY: Duration = Duration::from_millis(80);

fn next_startup_recovery_delay(current: Duration) -> Duration {
    current
        .checked_mul(2)
        .unwrap_or(STARTUP_RECOVERY_MAX_DELAY)
        .min(STARTUP_RECOVERY_MAX_DELAY)
}

/// Keep repairing an unavailable initial data plane without blocking panel
/// mutations on subscription network I/O. Foreground subscription operations
/// advance `sub_refresh_generation`, so their result always wins.
async fn retry_failed_startup(state: &Arc<AppState>) {
    let mut delay = STARTUP_RECOVERY_INITIAL_DELAY;
    loop {
        tokio::time::sleep(delay).await;

        if !should_retry_failed_startup(state).await {
            return;
        }
        if is_sing_box_running(state).await && state.runtime_ready.load(Ordering::Relaxed) {
            state.set_runtime_phase(RuntimePhase::Ready);
            return;
        }

        let config = state.config.read().await.clone();
        let refresh_generation = state.sub_refresh_generation.load(Ordering::Relaxed);
        if state.runtime_phase() == RuntimePhase::Failed {
            state.set_runtime_phase(if config.subs.is_empty() {
                RuntimePhase::Validating
            } else {
                RuntimePhase::FetchingSubscriptions
            });
        }
        let nodes =
            fetch_sub_nodes_if_current(&config, state, SubFetchRetry::Startup, refresh_generation)
                .await;

        let _config_update = state.config_update.lock().await;
        if !state.service_should_run.load(Ordering::Relaxed) {
            return;
        }
        if state.sub_refresh_generation.load(Ordering::Relaxed) != refresh_generation {
            info!("Startup recovery was superseded by a foreground subscription operation");
            if state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await {
                state.set_runtime_phase(RuntimePhase::Ready);
                return;
            }
            delay = next_startup_recovery_delay(delay);
            continue;
        }

        let current = state.config.read().await.clone();
        if current.subs != config.subs {
            info!("Subscriptions changed during startup recovery; discarding stale fetch");
            delay = STARTUP_RECOVERY_INITIAL_DELAY;
            continue;
        }
        if state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await {
            state.set_runtime_phase(RuntimePhase::Ready);
            return;
        }

        match refresh_subscriptions(
            &current,
            state,
            RefreshPolicy::Startup,
            SubSource::Prefetched(nodes),
        )
        .await
        {
            Ok(outcome) if outcome.effect == RefreshEffect::Activated => {
                info!(?outcome.runtime_update, "Initial data plane recovered in the background");
                save_config_cache(state).await;
                *state.config_warning.lock().await =
                    if !current.node_select.is_manual() && outcome.node_select.is_manual() {
                        Some("该地区没有可用节点，已切回手动选择".to_string())
                    } else {
                        None
                    };
                spawn_restore_last_proxy(state);
                return;
            }
            Ok(outcome) if outcome.effect == RefreshEffect::SkippedUnchanged => {
                if state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await {
                    save_config_cache(state).await;
                    state.set_runtime_phase(RuntimePhase::Ready);
                    return;
                }
                warn!("Startup recovery produced unchanged bytes without a ready data plane");
            }
            Ok(outcome) if outcome.effect == RefreshEffect::KeptRunningOnTotalFailure => {
                warn!("Startup recovery still cannot fetch any subscription nodes");
                *state.config_warning.lock().await =
                    Some("所有订阅获取失败，网络恢复后将自动重试".to_string());
            }
            Ok(outcome) if outcome.effect == RefreshEffect::KeptRunningOnValidationFailure => {
                error!("Startup recovery generated an invalid configuration");
                *state.config_warning.lock().await =
                    Some("订阅配置校验失败，修复订阅后将自动重试".to_string());
            }
            Ok(_) => {}
            Err(err) => {
                warn!(error = %err, "Startup data-plane recovery attempt failed");
                *state.config_warning.lock().await =
                    Some("代理服务仍未就绪，正在后台自动重试".to_string());
            }
        }
        if !state.runtime_ready.load(Ordering::Relaxed) {
            state.set_runtime_phase(RuntimePhase::Failed);
        }
        delay = next_startup_recovery_delay(delay);
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        initialize_runtime_locked, panel_bind_addr, prepare_compatible_startup_cache, spawn_server,
        RuntimeOptions,
    };

    #[tokio::test]
    async fn incompatible_cache_is_rejected_before_it_replaces_active_config() {
        use crate::{models::Config, services::config::save_config_cache, test_support::app_state};

        let original = Config {
            subs: vec!["https://old.example/sub".to_string()],
            ..Config::default()
        };
        let state = app_state(original);
        tokio::fs::create_dir_all(&state.runtime_paths.runtime_dir)
            .await
            .unwrap();
        tokio::fs::write(&state.runtime_paths.active_config, br#"{"outbounds":[]}"#)
            .await
            .unwrap();
        save_config_cache(&state).await;

        let active_before_fallback = br#"{"marker":"active-before-fallback"}"#;
        tokio::fs::write(&state.runtime_paths.active_config, active_before_fallback)
            .await
            .unwrap();
        let changed = Config {
            subs: vec!["https://new.example/sub".to_string()],
            ..Config::default()
        };

        let result = prepare_compatible_startup_cache(&changed, &state).await;

        assert!(result.is_err());
        assert_eq!(
            tokio::fs::read(&state.runtime_paths.active_config)
                .await
                .unwrap(),
            active_before_fallback
        );
        let _ = tokio::fs::remove_dir_all(&state.runtime_paths.runtime_dir).await;
    }

    #[cfg(unix)]
    async fn local_startup_test_state(
        config: crate::models::Config,
        label: &str,
    ) -> (std::sync::Arc<crate::state::AppState>, PathBuf) {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "miao-local-startup-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let runtime_dir = root.join("runtime");
        tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
        let kernel = runtime_dir.join("sing-box");
        tokio::fs::write(
            &kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
        std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

        let config_path = root.join("config.yaml");
        let volatile_path = root.join("volatile.yaml");
        let paths = crate::paths::RuntimePaths::new(runtime_dir, &config_path);
        let state = std::sync::Arc::new(
            crate::state::AppState::with_config_layers(
                crate::models::StableConfig::from(&config),
                config,
                config_path,
                volatile_path,
                paths,
            )
            .unwrap(),
        );
        (state, root)
    }

    async fn subscription_server(
        accepted: Option<std::sync::Arc<tokio::sync::Notify>>,
        release: Option<std::sync::Arc<tokio::sync::Notify>>,
    ) -> String {
        use axum::{routing::get, Router};

        const BODY: &str = r#"
proxies:
  - name: recovered-sub-node
    type: hysteria2
    server: 127.0.0.1
    port: 443
    password: secret
"#;
        let app = Router::new().route(
            "/sub",
            get(move || {
                let accepted = accepted.clone();
                let release = release.clone();
                async move {
                    if let Some(accepted) = accepted {
                        accepted.notify_one();
                    }
                    if let Some(release) = release {
                        release.notified().await;
                    }
                    BODY
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}/sub")
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn manual_nodes_start_before_any_subscription_request() {
        let config = crate::models::Config {
            subs: vec!["http://127.0.0.1:9/unreachable".to_string()],
            nodes: vec![serde_json::json!({
                "type": "hysteria2",
                "tag": "manual-local",
                "server": "127.0.0.1",
                "server_port": 443,
                "password": "secret"
            })
            .to_string()],
            ..crate::models::Config::default()
        };
        let (state, root) = local_startup_test_state(config.clone(), "manual").await;

        let needs_refresh = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            initialize_runtime_locked(&config, &state),
        )
        .await
        .expect("local startup must not wait for the unreachable subscription");

        assert!(needs_refresh);
        assert!(state
            .runtime_ready
            .load(std::sync::atomic::Ordering::Relaxed));
        assert!(state.sub_status.lock().await.is_empty());
        assert!(state.runtime_paths.config_cache.exists());

        crate::services::singbox::stop_sing_internal(&state).await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn legacy_manual_cache_requests_local_background_reconciliation() {
        let config = crate::models::Config {
            nodes: vec![serde_json::json!({
                "type": "hysteria2",
                "tag": "legacy-manual",
                "server": "127.0.0.1",
                "server_port": 443,
                "password": "secret"
            })
            .to_string()],
            ..crate::models::Config::default()
        };
        let (state, root) = local_startup_test_state(config.clone(), "legacy-manual").await;
        let outcome = crate::services::config::gen_config_from_nodes(&config, &state, Vec::new())
            .await
            .unwrap();
        crate::services::config::install_prepared_runtime(&state, &outcome)
            .await
            .unwrap();
        crate::services::config::save_config_cache(&state).await;
        tokio::fs::remove_file(&state.runtime_paths.cache_manifest)
            .await
            .unwrap();

        let needs_reconciliation = initialize_runtime_locked(&config, &state).await;

        assert!(needs_reconciliation);
        assert!(state
            .runtime_ready
            .load(std::sync::atomic::Ordering::Relaxed));

        crate::services::singbox::stop_sing_internal(&state).await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn matching_node_snapshot_starts_before_any_subscription_request() {
        let subscription = "http://127.0.0.1:9/unreachable".to_string();
        let config = crate::models::Config {
            subs: vec![subscription.clone()],
            ..crate::models::Config::default()
        };
        let (state, root) = local_startup_test_state(config.clone(), "snapshot").await;
        let snapshot = serde_json::json!({
            "version": 1,
            "subs": [subscription],
            "node_names": ["snapshot-local"],
            "outbounds": [{
                "type": "hysteria2",
                "tag": "snapshot-local",
                "server": "127.0.0.1",
                "server_port": 443,
                "password": "secret"
            }],
            "source_ids": ["snapshot-source"]
        });
        tokio::fs::write(
            &state.runtime_paths.sub_nodes_snapshot,
            serde_json::to_vec(&snapshot).unwrap(),
        )
        .await
        .unwrap();

        let needs_refresh = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            initialize_runtime_locked(&config, &state),
        )
        .await
        .expect("snapshot startup must not wait for the unreachable subscription");

        assert!(needs_refresh);
        assert!(state
            .runtime_ready
            .load(std::sync::atomic::Ordering::Relaxed));
        assert!(state.sub_status.lock().await.is_empty());
        let active = tokio::fs::read_to_string(&state.runtime_paths.active_config)
            .await
            .unwrap();
        assert!(active.contains("snapshot-local"));

        crate::services::singbox::stop_sing_internal(&state).await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_initial_start_keeps_retrying_until_the_data_plane_recovers() {
        use std::sync::atomic::Ordering;

        let subscription = subscription_server(None, None).await;
        let config = crate::models::Config {
            subs: vec![subscription],
            ..crate::models::Config::default()
        };
        let (state, root) = local_startup_test_state(config, "retry-recovery").await;
        state.initializing.store(false, Ordering::Relaxed);
        state.set_runtime_phase(crate::models::RuntimePhase::Failed);

        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            super::retry_failed_startup(&state),
        )
        .await
        .expect("background startup recovery must succeed");

        assert!(state.runtime_ready.load(Ordering::Relaxed));
        assert_eq!(state.runtime_phase(), crate::models::RuntimePhase::Ready);
        assert!(state.runtime_paths.config_cache.exists());
        assert_eq!(
            state
                .sub_status
                .lock()
                .await
                .values()
                .next()
                .map(|status| status.node_count),
            Some(1)
        );

        crate::services::singbox::stop_sing_internal(&state).await;
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn foreground_refresh_supersedes_an_older_startup_fetch() {
        use std::sync::atomic::Ordering;

        let accepted = std::sync::Arc::new(tokio::sync::Notify::new());
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let subscription = subscription_server(Some(accepted.clone()), Some(release.clone())).await;
        let config = crate::models::Config {
            subs: vec![subscription.clone()],
            ..crate::models::Config::default()
        };
        let (state, root) = local_startup_test_state(config.clone(), "refresh-generation").await;
        let active_before = br#"{"marker":"foreground-runtime"}"#;
        tokio::fs::write(&state.runtime_paths.active_config, active_before)
            .await
            .unwrap();
        state.runtime_ready.store(true, Ordering::Relaxed);
        state.set_runtime_phase(crate::models::RuntimePhase::Ready);

        let background_state = state.clone();
        let background = tokio::spawn(async move {
            super::refresh_subscriptions_in_background(&config, &background_state).await;
        });
        accepted.notified().await;

        state.sub_refresh_generation.fetch_add(1, Ordering::Relaxed);
        state.sub_status.lock().await.insert(
            subscription.clone(),
            crate::models::SubStatus {
                url: subscription,
                success: true,
                node_count: 99,
                state: crate::models::SubscriptionState::Ready,
                error: None,
            },
        );
        state.set_runtime_phase(crate::models::RuntimePhase::Ready);
        release.notify_one();
        background.await.unwrap();

        assert_eq!(
            tokio::fs::read(&state.runtime_paths.active_config)
                .await
                .unwrap(),
            active_before
        );
        assert_eq!(
            state
                .sub_status
                .lock()
                .await
                .values()
                .next()
                .map(|status| status.node_count),
            Some(99)
        );

        let _ = tokio::fs::remove_dir_all(root).await;
    }

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
            volatile_path: Some(unique_test_volatile_path()),
            skip_extract: true,
            runtime_dir: Some(unique_test_runtime_dir()),
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
            volatile_path: Some(unique_test_volatile_path()),
            skip_extract: true,
            runtime_dir: Some(unique_test_runtime_dir()),
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
            volatile_path: Some(unique_test_volatile_path()),
            skip_extract: true,
            runtime_dir: Some(unique_test_runtime_dir()),
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

    fn unique_test_volatile_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "miao-spawn-server-test-volatile-{}-{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ))
    }

    fn unique_test_runtime_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "miao-spawn-server-runtime-{}-{}",
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
