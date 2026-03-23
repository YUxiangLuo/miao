use std::path::PathBuf;
use tracing::{info, warn};

use crate::error::{AppError, AppResult};

const DEPS: &[&str] = &["kmod-tun", "kmod-nft-queue"];

#[derive(Clone, Copy, Debug)]
enum OpenWrtPm {
    /// OpenWrt 新版（如 25.x 起）使用 Alpine 系 apk。
    Apk,
    /// 旧版 opkg。
    Opkg,
}

/// apk 是否可用（不同构建可能支持 `-V` 或 `--version`）。
async fn apk_binary_responds() -> bool {
    for flag in ["-V", "--version"] {
        if let Ok(o) = tokio::process::Command::new("apk").arg(flag).output().await {
            if o.status.success() {
                return true;
            }
        }
    }
    false
}

/// 在 OpenWrt 上优先检测 apk，否则回退 opkg。
async fn detect_openwrt_pm() -> Option<OpenWrtPm> {
    if apk_binary_responds().await {
        return Some(OpenWrtPm::Apk);
    }
    let opkg_ok = tokio::process::Command::new("opkg")
        .arg("list-installed")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if opkg_ok {
        return Some(OpenWrtPm::Opkg);
    }
    None
}

async fn apk_package_installed(name: &str) -> bool {
    tokio::process::Command::new("apk")
        .args(["info", "-e", name])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn install_with_apk() -> AppResult<()> {
    let mut need: Vec<&str> = Vec::new();
    for pkg in DEPS {
        if !apk_package_installed(pkg).await {
            need.push(pkg);
        }
    }
    if need.is_empty() {
        info!("Required dependencies ({}) are already installed (apk).", DEPS.join(", "));
        return Ok(());
    }
    info!("Missing dependencies (apk): {:?}. Installing...", need);

    let update_status = tokio::process::Command::new("apk")
        .arg("update")
        .status()
        .await
        .map_err(|e| AppError::context("Failed to run 'apk update'", e))?;
    if !update_status.success() {
        warn!("'apk update' finished with error, but proceeding with installation attempt...");
    }

    for pkg in need {
        info!("Installing {} (apk)...", pkg);
        let st = tokio::process::Command::new("apk")
            .arg("add")
            .arg(pkg)
            .status()
            .await
            .map_err(|e| AppError::context(format!("Failed to run 'apk add {}'", pkg), e))?;
        if !st.success() {
            return Err(AppError::message(format!(
                "Failed to install {} (apk). Please install it manually.",
                pkg
            )));
        }
    }
    info!("Dependencies installed successfully (apk).");
    Ok(())
}

async fn install_with_opkg() -> AppResult<()> {
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
    for pkg in DEPS {
        if !installed_set.contains(pkg) {
            packages_to_install.push(*pkg);
        }
    }

    if packages_to_install.is_empty() {
        info!(
            "Required dependencies ({}) are already installed (opkg).",
            DEPS.join(", ")
        );
        return Ok(());
    }

    info!(
        "Missing dependencies (opkg): {:?}. Installing...",
        packages_to_install
    );

    info!("Running 'opkg update'...");
    let update_status = tokio::process::Command::new("opkg")
        .arg("update")
        .status()
        .await
        .map_err(|e| AppError::context("Failed to run 'opkg update'", e))?;

    if !update_status.success() {
        warn!(
            "'opkg update' finished with error, but proceeding with installation attempt..."
        );
    }

    for pkg in packages_to_install {
        info!("Installing {} (opkg)...", pkg);
        let install_status = tokio::process::Command::new("opkg")
            .arg("install")
            .arg(pkg)
            .status()
            .await
            .map_err(|e| AppError::context(format!("Failed to run 'opkg install {}'", pkg), e))?;

        if !install_status.success() {
            return Err(AppError::message(format!(
                "Failed to install {} (opkg). Please install it manually.",
                pkg
            )));
        }
    }

    info!("Dependencies installed successfully (opkg).");
    Ok(())
}

pub async fn check_and_install_openwrt_dependencies() -> AppResult<()> {
    if !PathBuf::from("/etc/openwrt_release").exists() {
        return Ok(());
    }

    info!("OpenWrt system detected. Checking dependencies...");

    match detect_openwrt_pm().await {
        Some(OpenWrtPm::Apk) => {
            info!("Using apk package manager.");
            install_with_apk().await
        }
        Some(OpenWrtPm::Opkg) => {
            info!("Using opkg package manager.");
            install_with_opkg().await
        }
        None => {
            warn!(
                "Neither apk nor opkg responded; skip automatic dependency install. \
                 Install kmod-tun and kmod-nft-queue manually if needed."
            );
            Ok(())
        }
    }
}
