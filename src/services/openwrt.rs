use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};

enum PkgManager {
    Apk(PathBuf),
    Opkg(PathBuf),
}

impl PkgManager {
    fn detect() -> Option<Self> {
        Self::detect_in_dirs(command_search_dirs())
    }

    fn detect_in_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Option<Self> {
        let dirs: Vec<PathBuf> = dirs.into_iter().collect();
        find_command(&dirs, "apk")
            .map(Self::Apk)
            .or_else(|| find_command(&dirs, "opkg").map(Self::Opkg))
    }

    fn name(&self) -> &str {
        match self {
            Self::Apk(_) => "apk",
            Self::Opkg(_) => "opkg",
        }
    }

    fn command_path(&self) -> &Path {
        match self {
            Self::Apk(path) | Self::Opkg(path) => path.as_path(),
        }
    }

    async fn is_installed(&self, pkg: &str) -> AppResult<bool> {
        match self {
            Self::Apk(_) => {
                // apk info -e <pkg> 返回 0 表示已安装
                let status = tokio::process::Command::new(self.command_path())
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
            Self::Opkg(_) => {
                let output = tokio::process::Command::new(self.command_path())
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
            Self::Apk(_) => {
                tokio::process::Command::new(self.command_path())
                    .arg("update")
                    .status()
                    .await
            }
            Self::Opkg(_) => {
                tokio::process::Command::new(self.command_path())
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
            Self::Apk(_) => {
                tokio::process::Command::new(self.command_path())
                    .args(["add", pkg])
                    .status()
                    .await
            }
            Self::Opkg(_) => {
                tokio::process::Command::new(self.command_path())
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

fn command_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();

    for path in ["/sbin", "/usr/sbin", "/bin", "/usr/bin"] {
        let path = PathBuf::from(path);
        if !dirs.iter().any(|existing| existing == &path) {
            dirs.push(path);
        }
    }

    dirs
}

fn find_command(dirs: &[PathBuf], command: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(command))
        .find(|path| path.exists())
}

async fn ensure_tun_device_available() -> AppResult<()> {
    let tun_device = PathBuf::from("/dev/net/tun");
    if tun_device.exists() {
        return Ok(());
    }

    let Some(modprobe) = find_command(&command_search_dirs(), "modprobe") else {
        return Err(AppError::message(
            "/dev/net/tun is missing and modprobe was not found; install/load kmod-tun manually",
        ));
    };

    info!("/dev/net/tun is missing, trying to load tun module...");
    let status = tokio::process::Command::new(&modprobe)
        .arg("tun")
        .status()
        .await
        .map_err(|e| AppError::context("Failed to run modprobe tun", e))?;

    if !status.success() {
        return Err(AppError::message(format!(
            "modprobe tun failed with status {status}; /dev/net/tun is still unavailable"
        )));
    }

    if !tun_device.exists() {
        return Err(AppError::message(
            "modprobe tun succeeded but /dev/net/tun is still missing",
        ));
    }

    Ok(())
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
        ensure_tun_device_available().await?;
        return Ok(());
    }

    info!("Missing dependencies: {:?}. Installing...", missing);

    pm.update_index().await?;

    for pkg in missing {
        pm.install(pkg).await?;
    }

    info!("Dependencies installed successfully.");
    ensure_tun_device_available().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::PkgManager;

    fn temp_command_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("miao-openwrt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn detects_apk_from_bin_style_dir() {
        let dir = temp_command_dir("apk-bin");
        touch(&dir.join("apk"));

        let pm = PkgManager::detect_in_dirs([dir.clone()]).unwrap();

        assert!(matches!(pm, PkgManager::Apk(_)));
        assert_eq!(pm.command_path(), dir.join("apk").as_path());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prefers_apk_when_both_package_managers_exist() {
        let dir = temp_command_dir("apk-opkg");
        touch(&dir.join("apk"));
        touch(&dir.join("opkg"));

        let pm = PkgManager::detect_in_dirs([dir.clone()]).unwrap();

        assert!(matches!(pm, PkgManager::Apk(_)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn detects_opkg_when_apk_is_missing() {
        let dir = temp_command_dir("opkg");
        touch(&dir.join("opkg"));

        let pm = PkgManager::detect_in_dirs([dir.clone()]).unwrap();

        assert!(matches!(pm, PkgManager::Opkg(_)));
        assert_eq!(pm.command_path(), dir.join("opkg").as_path());
        let _ = std::fs::remove_dir_all(dir);
    }
}
