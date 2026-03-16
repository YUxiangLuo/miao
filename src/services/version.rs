use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    sync::LazyLock,
};

use tokio::sync::RwLock;
use tokio::time::{sleep, Duration, Instant};

use crate::error::{AppError, AppResult};
use crate::models::{GitHubRelease, VersionInfo};
use crate::services::singbox::{get_sing_box_home, stop_sing_internal};
use crate::VERSION;

const CACHE_TTL: Duration = Duration::from_secs(300);

struct ReleaseCache {
    release: Option<GitHubRelease>,
    fetched_at: Option<Instant>,
}

static RELEASE_CACHE: LazyLock<RwLock<ReleaseCache>> = LazyLock::new(|| {
    RwLock::new(ReleaseCache {
        release: None,
        fetched_at: None,
    })
});

fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let v = v.strip_prefix('v').unwrap_or(v);
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

fn current_version() -> String {
    format!("v{}", VERSION)
}

fn current_arch_asset_name() -> Option<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Some("miao-rust-linux-amd64")
    } else if cfg!(target_arch = "aarch64") {
        Some("miao-rust-linux-arm64")
    } else {
        None
    }
}

async fn fetch_latest_release_uncached() -> AppResult<GitHubRelease> {
    let release = crate::state::CLIENT
        .get("https://api.github.com/repos/YUxiangLuo/miao/releases/latest")
        .timeout(Duration::from_secs(60))
        .header("User-Agent", "miao")
        .send()
        .await?
        .json::<GitHubRelease>()
        .await?;

    Ok(release)
}

async fn fetch_latest_release() -> AppResult<GitHubRelease> {
    {
        let cache = RELEASE_CACHE.read().await;
        if let (Some(release), Some(fetched_at)) = (&cache.release, cache.fetched_at) {
            if fetched_at.elapsed() < CACHE_TTL {
                return Ok(release.clone());
            }
        }
    }

    let release = fetch_latest_release_uncached().await?;
    {
        let mut cache = RELEASE_CACHE.write().await;
        cache.release = Some(release.clone());
        cache.fetched_at = Some(Instant::now());
    }
    Ok(release)
}

async fn invalidate_release_cache() {
    let mut cache = RELEASE_CACHE.write().await;
    cache.release = None;
    cache.fetched_at = None;
}

pub async fn get_version_info() -> VersionInfo {
    let current = current_version();
    let asset_name = current_arch_asset_name().unwrap_or("");

    match fetch_latest_release().await {
        Ok(release) => {
            let latest = release.tag_name.clone();
            let has_update = is_newer_version(&current, &latest);
            let download_url = release
                .assets
                .iter()
                .find(|a| a.name == asset_name)
                .map(|a| a.browser_download_url.clone());

            VersionInfo {
                current,
                latest: Some(latest),
                has_update,
                download_url,
            }
        }
        Err(_) => VersionInfo {
            current,
            latest: None,
            has_update: false,
            download_url: None,
        },
    }
}

pub async fn upgrade_binary() -> AppResult<String> {
    // Force fresh fetch for upgrade to ensure we have the latest info
    invalidate_release_cache().await;
    let release = fetch_latest_release().await?;
    let current = current_version();

    if !is_newer_version(&current, &release.tag_name) {
        return Ok("Already up to date".to_string());
    }

    let asset_name =
        current_arch_asset_name().ok_or_else(|| AppError::message("Unsupported architecture"))?;
    let download_url = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .map(|a| a.browser_download_url.clone())
        .ok_or_else(|| AppError::message("No binary found for current architecture"))?;

    println!("Downloading update from: {}", download_url);
    let binary_data = crate::state::CLIENT
        .get(&download_url)
        .timeout(Duration::from_secs(60))
        .send()
        .await?
        .bytes()
        .await?;

    let temp_path = "/tmp/miao-new";
    fs::write(temp_path, &binary_data)
        .map_err(|e| AppError::context("Failed to write temp file", e))?;
    fs::set_permissions(temp_path, fs::Permissions::from_mode(0o755))
        .map_err(|e| AppError::context("Failed to set permissions on temp file", e))?;

    // Verify the new binary is a valid miao binary by checking --version output
    let verify = tokio::process::Command::new(temp_path)
        .arg("--version")
        .output()
        .await;
    match verify {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.contains("miao") && !stdout.contains(&release.tag_name) {
                let _ = fs::remove_file(temp_path);
                return Err(AppError::message("New binary verification failed: unexpected --version output"));
            }
        }
        Err(_) => {
            let _ = fs::remove_file(temp_path);
            return Err(AppError::message("New binary verification failed"));
        }
    }

    let current_exe = std::env::current_exe()?;

    println!("Stopping sing-box before upgrade...");
    stop_sing_internal().await;

    // Use rename for atomic replacement instead of remove + copy
    let backup_path = format!("{}.bak", current_exe.display());
    fs::rename(&current_exe, &backup_path)
        .map_err(|e| AppError::context("Failed to backup current binary", e))?;

    if let Err(e) = fs::copy(temp_path, &current_exe) {
        // Restore from backup on failure
        let _ = fs::rename(&backup_path, &current_exe);
        return Err(AppError::context("Failed to install new binary", e));
    }
    if let Err(e) = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755)) {
        let _ = fs::remove_file(&current_exe);
        let _ = fs::rename(&backup_path, &current_exe);
        return Err(AppError::context("Failed to set permissions on new binary", e));
    }
    let _ = fs::remove_file(temp_path);

    println!("Upgrade successful! Restarting...");

    let new_version = release.tag_name.clone();
    let sing_box_home = get_sing_box_home();
    tokio::spawn(async move {
        sleep(Duration::from_millis(500)).await;

        let files_to_remove = ["sing-box", "chinaip.srs", "chinasite.srs"];
        for file in &files_to_remove {
            let path = sing_box_home.join(file);
            if path.exists() {
                println!("Removing old file: {:?}", path);
                let _ = fs::remove_file(&path);
            }
        }

        let args: Vec<String> = std::env::args().collect();
        let err = std::process::Command::new(&current_exe)
            .args(&args[1..])
            .exec();

        eprintln!("Failed to exec new binary: {}", err);
        eprintln!("Attempting to restore from backup...");

        if fs::rename(&backup_path, &current_exe).is_ok() {
            let _ = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755));
            eprintln!("Restored from backup, restarting with old version...");
            let _ = std::process::Command::new(&current_exe)
                .args(&args[1..])
                .exec();
        }
        eprintln!("Failed to restore from backup, manual intervention required");
        std::process::exit(1);
    });

    Ok(new_version)
}

#[cfg(test)]
mod tests {
    use super::{current_arch_asset_name, is_newer_version, parse_version};

    #[test]
    fn parse_version_accepts_prefixed_and_unprefixed_versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
    }

    #[test]
    fn parse_version_rejects_invalid_shapes() {
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3.4"), None);
        assert_eq!(parse_version("vx.y.z"), None);
    }

    #[test]
    fn is_newer_version_compares_semver_parts_correctly() {
        assert!(is_newer_version("v0.9.9", "v0.10.0"));
        assert!(is_newer_version("v1.2.9", "v1.3.0"));
        assert!(!is_newer_version("v1.0.0", "v1.0.0"));
        assert!(!is_newer_version("v2.0.0", "v1.9.9"));
    }

    #[test]
    fn current_arch_asset_name_matches_supported_targets() {
        if cfg!(target_arch = "x86_64") {
            assert_eq!(current_arch_asset_name(), Some("miao-rust-linux-amd64"));
        } else if cfg!(target_arch = "aarch64") {
            assert_eq!(current_arch_asset_name(), Some("miao-rust-linux-arm64"));
        } else {
            assert_eq!(current_arch_asset_name(), None);
        }
    }
}
