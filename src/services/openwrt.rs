use std::path::PathBuf;
use tracing::{info, warn};

use crate::error::{AppError, AppResult};

enum PkgManager {
    Apk,
    Opkg,
}

impl PkgManager {
    fn detect() -> Option<Self> {
        if PathBuf::from("/sbin/apk").exists() || PathBuf::from("/usr/sbin/apk").exists() {
            Some(Self::Apk)
        } else if PathBuf::from("/bin/opkg").exists() || PathBuf::from("/usr/bin/opkg").exists() {
            Some(Self::Opkg)
        } else {
            None
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Apk => "apk",
            Self::Opkg => "opkg",
        }
    }

    async fn is_installed(&self, pkg: &str) -> AppResult<bool> {
        match self {
            Self::Apk => {
                // apk info -e <pkg> 返回 0 表示已安装
                let status = tokio::process::Command::new("apk")
                    .args(["info", "-e", pkg])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .await
                    .map_err(|e| {
                        AppError::context(format!("Failed to check package '{}' via apk", pkg), e)
                    })?;
                Ok(status.success())
            }
            Self::Opkg => {
                let output = tokio::process::Command::new("opkg")
                    .args(["status", pkg])
                    .output()
                    .await
                    .map_err(|e| {
                        AppError::context(format!("Failed to check package '{}' via opkg", pkg), e)
                    })?;
                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(stdout.contains("Status:") && stdout.contains("installed"))
            }
        }
    }

    async fn update_index(&self) -> AppResult<()> {
        info!("Running '{} update'...", self.name());
        let status = match self {
            Self::Apk => {
                tokio::process::Command::new("apk")
                    .arg("update")
                    .status()
                    .await
            }
            Self::Opkg => {
                tokio::process::Command::new("opkg")
                    .arg("update")
                    .status()
                    .await
            }
        }
        .map_err(|e| AppError::context(format!("Failed to run '{} update'", self.name()), e))?;

        if !status.success() {
            warn!(
                "'{} update' finished with error, but proceeding with installation attempt...",
                self.name()
            );
        }
        Ok(())
    }

    async fn install(&self, pkg: &str) -> AppResult<()> {
        info!("Installing {} via {}...", pkg, self.name());
        let status = match self {
            Self::Apk => {
                tokio::process::Command::new("apk")
                    .args(["add", pkg])
                    .status()
                    .await
            }
            Self::Opkg => {
                tokio::process::Command::new("opkg")
                    .args(["install", pkg])
                    .status()
                    .await
            }
        }
        .map_err(|e| {
            AppError::context(
                format!("Failed to run '{} install {}'", self.name(), pkg),
                e,
            )
        })?;

        if !status.success() {
            return Err(AppError::message(format!(
                "Failed to install {} via {}. Please install it manually.",
                pkg,
                self.name()
            )));
        }
        Ok(())
    }
}

pub async fn check_and_install_openwrt_dependencies() -> AppResult<()> {
    if !PathBuf::from("/etc/openwrt_release").exists() {
        return Ok(());
    }

    info!("OpenWrt system detected. Checking dependencies...");

    let pm = PkgManager::detect().ok_or_else(|| {
        AppError::message("OpenWrt detected but neither apk nor opkg found".to_string())
    })?;
    info!("Using package manager: {}", pm.name());

    let required = ["kmod-tun", "kmod-nft-queue"];
    let mut missing = Vec::new();
    for pkg in &required {
        if !pm.is_installed(pkg).await? {
            missing.push(*pkg);
        }
    }

    if missing.is_empty() {
        info!(
            "Required dependencies ({}) are already installed.",
            required.join(", ")
        );
        return Ok(());
    }

    info!("Missing dependencies: {:?}. Installing...", missing);

    pm.update_index().await?;

    for pkg in missing {
        pm.install(pkg).await?;
    }

    info!("Dependencies installed successfully.");
    Ok(())
}
