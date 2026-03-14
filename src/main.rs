mod error;
mod handlers;
mod models;
mod responses;
mod router;
mod services;
mod state;
#[cfg(test)]
mod test_support;

use crate::error::AppResult;
use nix::unistd::Uid;
use std::{fs, sync::Arc};
use tokio::time::{sleep, Duration};

use models::{Config, DEFAULT_PORT};
use services::{
    config::gen_config,
    openwrt::check_and_install_openwrt_dependencies,
    proxy::restore_last_proxy,
    singbox::{extract_sing_box, start_sing_internal},
};
use state::AppState;

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> AppResult<()> {
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

    println!("Generating initial config...");
    loop {
        match gen_config(&config).await {
            Ok(_) => break,
            Err(e) => {
                eprintln!(
                    "Failed to generate config: {}. Retrying in 300 seconds...",
                    e
                );
                sleep(Duration::from_secs(300)).await;
            }
        }
    }

    println!("Checking dependencies...");
    if let Err(e) = check_and_install_openwrt_dependencies().await {
        eprintln!("Failed to check or install OpenWrt dependencies: {}", e);
    }

    match start_sing_internal().await {
        Ok(_) => {
            println!("sing-box started successfully");
            tokio::spawn(async {
                restore_last_proxy().await;
            });
        }
        Err(e) => eprintln!("Failed to start sing-box: {}", e),
    }

    let app_state = Arc::new(AppState {
        config: tokio::sync::Mutex::new(config.clone()),
    });
    let app = router::build_router(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("✅ Miao 控制面板已启动: http://localhost:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
