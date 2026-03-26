use std::path::PathBuf;
use tracing::{info, warn};

use crate::error::{AppError, AppResult};

pub async fn check_and_install_openwrt_dependencies() -> AppResult<()> {
    if !PathBuf::from("/etc/openwrt_release").exists() {
        return Ok(());
    }

    info!("OpenWrt system detected. Checking dependencies...");

    let output = tokio::process::Command::new("opkg")
        .arg("list-installed")
        .output()
        .await
        .map_err(|e| AppError::context("Failed to query installed OpenWrt packages", e))?;

    let installed_list = String::from_utf8_lossy(&output.stdout);
    let installed_set: std::collections::HashSet<&str> = installed_list
        .lines()
        .map(|line| line.split_whitespace().next().unwrap_or(""))
        .collect();

    let mut packages_to_install = Vec::new();

    if !installed_set.contains("kmod-tun") {
        packages_to_install.push("kmod-tun");
    }
    if !installed_set.contains("kmod-nft-queue") {
        packages_to_install.push("kmod-nft-queue");
    }

    if packages_to_install.is_empty() {
        info!("Required dependencies (kmod-tun, kmod-nft-queue) are already installed.");
        return Ok(());
    }

    info!(
        "Missing dependencies: {:?}. Installing...",
        packages_to_install
    );

    info!("Running 'opkg update'...");
    let update_status = tokio::process::Command::new("opkg")
        .arg("update")
        .status()
        .await
        .map_err(|e| AppError::context("Failed to run 'opkg update'", e))?;

    if !update_status.success() {
        warn!("'opkg update' finished with error, but proceeding with installation attempt...");
    }

    for pkg in packages_to_install {
        info!("Installing {}...", pkg);
        let install_status = tokio::process::Command::new("opkg")
            .arg("install")
            .arg(pkg)
            .status()
            .await
            .map_err(|e| AppError::context(format!("Failed to run 'opkg install {}'", pkg), e))?;

        if !install_status.success() {
            return Err(AppError::message(format!(
                "Failed to install {}. Please install it manually.",
                pkg
            )));
        }
    }

    info!("Dependencies installed successfully.");
    Ok(())
}
