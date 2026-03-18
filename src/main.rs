mod error;
mod handlers;
mod models;
mod responses;
mod router;
mod services;
mod state;
mod validation;
#[cfg(test)]
mod test_support;

use crate::error::AppResult;
use nix::unistd::Uid;
use std::{fs, sync::Arc};

use models::{Config, DEFAULT_PORT};
use services::{
    config::{gen_config, restore_config_from_cache, save_config_cache},
    openwrt::check_and_install_openwrt_dependencies,
    proxy::restore_last_proxy,
    singbox::{extract_sing_box, start_sing_internal, stop_sing_internal},
};
use state::AppState;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> AppResult<()> {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("miao v{}", VERSION);
        return Ok(());
    }

    if !Uid::effective().is_root() {
        eprintln!("Error: This application must be run as root.");
        std::process::exit(1);
    }

    if let Ok(current_exe) = std::env::current_exe() {
        let backup_path = format!("{}.bak", current_exe.display());
        if std::path::Path::new(&backup_path).exists() {
            let _ = fs::remove_file(&backup_path);
        }
    }

    println!("Reading configuration...");
    let config: Config = serde_yaml::from_str(&tokio::fs::read_to_string("config.yaml").await?)?;
    let port = config.port.unwrap_or(DEFAULT_PORT);

    let _ = extract_sing_box()?;

    // 初始化应用状态
    let app_state = Arc::new(AppState::new(config.clone()));
    let state_for_init = app_state.clone();

    // Start web server immediately so the panel is accessible during initialization
    let app = router::build_router(app_state.clone());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("✅ Miao 控制面板已启动: http://localhost:{}", port);

    // Background: generate config, check dependencies, and start sing-box
    tokio::spawn(async move {
        println!("Generating initial config...");
        let mut all_subs_failed = false;
        match gen_config(&config, &state_for_init).await {
            Ok(has_sub_nodes) => {
                if has_sub_nodes {
                    save_config_cache().await;
                } else if !config.subs.is_empty() {
                    all_subs_failed = true;
                }
            }
            Err(e) => {
                eprintln!("Failed to generate config: {}", e);
                match restore_config_from_cache().await {
                    Ok(_) => {
                        println!("Using cached config as fallback");
                        all_subs_failed = true;
                    }
                    Err(cache_err) => {
                        eprintln!("No cached config available: {}", cache_err);
                        *state_for_init.config_warning.lock().await = Some(
                            "所有订阅获取失败且无可用缓存，请添加订阅或手动节点".to_string()
                        );
                        state_for_init.initializing.store(false, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                }
            }
        }

        println!("Checking dependencies...");
        if let Err(e) = check_and_install_openwrt_dependencies().await {
            eprintln!("Failed to check or install OpenWrt dependencies: {}", e);
        }

        match start_sing_internal(&state_for_init).await {
            Ok(_) => {
                println!("sing-box started successfully");
                if all_subs_failed {
                    *state_for_init.config_warning.lock().await = Some(
                        "所有订阅获取失败，请检查当前订阅".to_string()
                    );
                }
                let state_for_proxy = state_for_init.clone();
                tokio::spawn(async move {
                    restore_last_proxy(&state_for_proxy).await;
                });
            }
            Err(e) => eprintln!("Failed to start sing-box: {}", e),
        }
        state_for_init.initializing.store(false, std::sync::atomic::Ordering::Relaxed);
    });

    let state_for_shutdown = app_state.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(state_for_shutdown))
        .await?;
    Ok(())
}

async fn shutdown_signal(state: Arc<AppState>) {
    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");

    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result.expect("failed to install Ctrl+C handler");
        }
        _ = sigterm.recv() => {}
    }

    println!("Shutting down, stopping sing-box...");
    stop_sing_internal(&state).await;
}
