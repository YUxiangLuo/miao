use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use futures::StreamExt;
use sha2::{Sha256, Digest};
use tokio::time::{sleep, Duration};
use tracing::{error, info};

use crate::error::{AppError, AppResult};
use crate::models::{GitHubRelease, GitHubAsset, VersionInfo};
use crate::services::singbox::{get_sing_box_home, stop_sing_internal};
use crate::state::{AppState, VersionCache};
use crate::VERSION;

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

const CACHE_TTL: Duration = Duration::from_secs(300);

async fn fetch_latest_release_uncached(client: &reqwest::Client) -> AppResult<GitHubRelease> {
    let release = client
        .get("https://api.github.com/repos/YUxiangLuo/miao/releases/latest")
        .timeout(Duration::from_secs(60))
        .header("User-Agent", "miao")
        .send()
        .await?
        .json::<GitHubRelease>()
        .await?;

    Ok(release)
}

async fn fetch_latest_release(
    client: &reqwest::Client,
    state: &Arc<AppState>,
) -> AppResult<GitHubRelease> {
    // 无锁读取缓存
    let cache = state.version_cache.load();
    if let (Some(release), Some(fetched_at)) = (&cache.release, cache.fetched_at) {
        if fetched_at.elapsed() < CACHE_TTL {
            return Ok(release.clone());
        }
    }
    // 释放读取 guard，避免持有期间进行网络请求
    drop(cache);

    let release = fetch_latest_release_uncached(client).await?;
    // 原子更新缓存
    state.version_cache.store(Arc::new(VersionCache {
        release: Some(release.clone()),
        fetched_at: Some(Instant::now()),
    }));
    Ok(release)
}

async fn invalidate_release_cache(state: &Arc<AppState>) {
    state.version_cache.store(Arc::new(VersionCache {
        release: None,
        fetched_at: None,
    }));
}

pub async fn get_version_info(state: &Arc<AppState>) -> VersionInfo {
    let current = current_version();
    let asset_name = current_arch_asset_name().unwrap_or("");

    match fetch_latest_release(&state.http_client, state).await {
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

fn get_temp_binary_path() -> String {
    let pid = std::process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("/tmp/miao-new-{}-{}", pid, timestamp)
}

pub async fn upgrade_binary(state: &Arc<AppState>) -> AppResult<String> {
    // 检查是否正在升级中，防止并发升级请求
    if state.upgrading.load(Ordering::SeqCst) {
        return Err(AppError::message("Upgrade already in progress"));
    }
    
    // 标记升级开始
    state.upgrading.store(true, Ordering::SeqCst);
    
    // 确保无论结果如何都会重置升级状态
    struct UpgradeGuard(Arc<AppState>);
    impl Drop for UpgradeGuard {
        fn drop(&mut self) {
            self.0.upgrading.store(false, Ordering::SeqCst);
        }
    }
    let _guard = UpgradeGuard(state.clone());
    
    // Force fresh fetch for upgrade to ensure we have the latest info
    invalidate_release_cache(state).await;
    let release = fetch_latest_release(&state.http_client, state).await?;
    let current = current_version();

    if !is_newer_version(&current, &release.tag_name) {
        return Ok("Already up to date".to_string());
    }

    let asset_name =
        current_arch_asset_name().ok_or_else(|| AppError::message("Unsupported architecture"))?;
    let asset: &GitHubAsset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| AppError::message("No binary found for current architecture"))?;
    
    let download_url = &asset.browser_download_url;
    let expected_size = asset.size;

    info!("Downloading update from: {} ({} bytes)", download_url, expected_size);
    
    // 使用流式下载并记录进度
    let response = state.http_client
        .get(download_url)
        .timeout(Duration::from_secs(120))  // 增加超时到 120s，适应大文件慢网络
        .send()
        .await?;
    
    let mut downloaded: u64 = 0;
    let mut last_logged_percent = 0u8;
    let mut binary_data = Vec::with_capacity(expected_size as usize);
    
    let mut stream = response.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk: bytes::Bytes = chunk_result.map_err(|e| AppError::context("Download stream error", e))?;
        downloaded += chunk.len() as u64;
        binary_data.extend_from_slice(&chunk);
        
        // 每 10% 记录一次进度
        if expected_size > 0 {
            let percent = ((downloaded as f64 / expected_size as f64) * 100.0) as u8;
            if percent >= last_logged_percent + 10 {
                info!("Download progress: {}% ({}/{} bytes)", percent, downloaded, expected_size);
                last_logged_percent = percent;
            }
        }
    }
    
    info!("Download complete: {} bytes", downloaded);

    // Verify file size
    let actual_size = binary_data.len() as u64;
    if actual_size != expected_size {
        return Err(AppError::message(format!(
            "Downloaded file size mismatch: expected {} bytes, got {} bytes",
            expected_size, actual_size
        )));
    }

    // Compute and log SHA256 for verification
    let sha256_hash = compute_sha256(&binary_data);
    info!("Downloaded binary SHA256: {}", sha256_hash);

    let temp_path = get_temp_binary_path();
    fs::write(&temp_path, &binary_data)
        .map_err(|e| AppError::context("Failed to write temp file", e))?;
    fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))
        .map_err(|e| AppError::context("Failed to set permissions on temp file", e))?;

    // Verify the new binary is a valid miao binary by checking --version output
    let verify = tokio::process::Command::new(&temp_path)
        .arg("--version")
        .output()
        .await;
    match verify {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // 检查是否包含版本号或程序名，使用更严格的验证
            let version_pattern = format!("v{}", &release.tag_name.trim_start_matches('v'));
            let has_valid_version = stdout.contains(&version_pattern) || 
                                   stdout.contains(&release.tag_name);
            let has_program_name = stdout.contains("miao") || stdout.contains("miao-rust");
            
            if !has_valid_version && !has_program_name {
                let _ = fs::remove_file(&temp_path);
                return Err(AppError::message(format!(
                    "New binary verification failed: unexpected --version output: {}", 
                    stdout.trim()
                )));
            }
        }
        Err(e) => {
            let _ = fs::remove_file(&temp_path);
            return Err(AppError::context("New binary verification failed", e));
        }
    }

    let current_exe = std::env::current_exe()?;

    info!("Stopping sing-box before upgrade...");
    stop_sing_internal(state).await;

    // Use rename for atomic replacement instead of remove + copy
    let backup_path = format!("{}.bak", current_exe.display());
    fs::rename(&current_exe, &backup_path)
        .map_err(|e| AppError::context("Failed to backup current binary", e))?;

    if let Err(e) = fs::copy(&temp_path, &current_exe) {
        // Restore from backup on failure
        let _ = fs::rename(&backup_path, &current_exe);
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::context("Failed to install new binary", e));
    }
    if let Err(e) = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755)) {
        let _ = fs::remove_file(&current_exe);
        let _ = fs::rename(&backup_path, &current_exe);
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::context("Failed to set permissions on new binary", e));
    }
    let _ = fs::remove_file(&temp_path);

    info!("Upgrade successful! Restarting...");

    let new_version = release.tag_name.clone();
    let sing_box_home = get_sing_box_home();
    tokio::spawn(async move {
        sleep(Duration::from_millis(500)).await;

        let files_to_remove = ["sing-box", "chinaip.srs", "chinasite.srs"];
        for file in &files_to_remove {
            let path = sing_box_home.join(file);
            if path.exists() {
                info!("Removing old file: {:?}", path);
                let _ = fs::remove_file(&path);
            }
        }

        let args: Vec<String> = std::env::args().collect();
        let err = std::process::Command::new(&current_exe)
            .args(&args[1..])
            .exec();

        error!("Failed to exec new binary: {}", err);
        error!("Attempting to restore from backup...");

        if fs::rename(&backup_path, &current_exe).is_ok() {
            let _ = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755));
            error!("Restored from backup, restarting with old version...");
            let _ = std::process::Command::new(&current_exe)
                .args(&args[1..])
                .exec();
        }
        error!("Failed to restore from backup, manual intervention required");
        std::process::exit(1);
    });

    Ok(new_version)
}

fn parse_semver(v: &str) -> Option<semver::Version> {
    let v = v.strip_prefix('v').unwrap_or(v);
    semver::Version::parse(v).ok()
}

fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
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

#[cfg(test)]
mod tests {
    use super::{current_arch_asset_name, is_newer_version, parse_semver};

    #[test]
    fn parse_semver_accepts_prefixed_and_unprefixed_versions() {
        assert!(parse_semver("v1.2.3").is_some());
        assert!(parse_semver("1.2.3").is_some());
    }

    #[test]
    fn parse_semver_handles_pre_release_versions() {
        // semver crate supports pre-release versions like 1.0.0-beta, 1.0.0-alpha.1
        assert!(parse_semver("v1.0.0-beta").is_some());
        assert!(parse_semver("v1.0.0-alpha.1").is_some());
        assert!(parse_semver("v1.0.0+build.123").is_some());
    }

    #[test]
    fn parse_semver_rejects_invalid_shapes() {
        assert!(parse_semver("v1.2").is_none());
        assert!(parse_semver("v1.2.3.4").is_none());
        assert!(parse_semver("vx.y.z").is_none());
    }

    #[test]
    fn is_newer_version_compares_semver_parts_correctly() {
        assert!(is_newer_version("v0.9.9", "v0.10.0"));
        assert!(is_newer_version("v1.2.9", "v1.3.0"));
        assert!(!is_newer_version("v1.0.0", "v1.0.0"));
        assert!(!is_newer_version("v2.0.0", "v1.9.9"));
    }

    #[test]
    fn is_newer_version_handles_pre_release() {
        // Pre-release versions are considered older than release versions
        assert!(is_newer_version("v1.0.0-beta", "v1.0.0"));
        assert!(!is_newer_version("v1.0.0", "v1.0.0-beta"));
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
