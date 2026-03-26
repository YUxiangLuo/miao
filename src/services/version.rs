use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Instant,
};

use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{GitHubAsset, GitHubRelease, VersionInfo};
use crate::services::singbox::{get_sing_box_home, stop_sing_internal};
use crate::state::{AppState, VersionCache};
use crate::VERSION;

const CACHE_TTL: Duration = Duration::from_secs(300);
const DOWNLOAD_MAX_ATTEMPTS: u32 = 3;
const DOWNLOAD_RETRY_BASE_MS: u64 = 500;

/// 解析 `sha256sum` 输出首行：`<64 hex>[  *]<filename>`
fn parse_sha256sum_line(line: &str) -> AppResult<String> {
    let line = line.trim();
    let hex = line
        .split_whitespace()
        .next()
        .ok_or_else(|| AppError::message("checksum file is empty"))?;
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::message(format!(
            "invalid SHA256 in checksum file (first token): {line}"
        )));
    }
    Ok(hex.to_ascii_lowercase())
}

async fn fetch_checksum_hex(client: &reqwest::Client, url: &str) -> AppResult<String> {
    let text = client
        .get(url)
        .timeout(Duration::from_secs(30))
        .header("User-Agent", "miao")
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AppError::context("Failed to download checksum file", e))?
        .text()
        .await
        .map_err(|e| AppError::context("Failed to read checksum body", e))?;

    let first = text.lines().next().unwrap_or("").trim();
    parse_sha256sum_line(first)
}

async fn fetch_checksum_hex_retried(client: &reqwest::Client, url: &str) -> AppResult<String> {
    let mut last_err: Option<AppError> = None;
    for attempt in 0..DOWNLOAD_MAX_ATTEMPTS {
        if attempt > 0 {
            sleep(Duration::from_millis(
                DOWNLOAD_RETRY_BASE_MS * (1 << (attempt - 1)),
            ))
            .await;
            warn!(
                attempt = attempt + 1,
                max = DOWNLOAD_MAX_ATTEMPTS,
                "retrying checksum download"
            );
        }
        match fetch_checksum_hex(client, url).await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.expect("checksum retry loop"))
}

/// 流式下载到临时文件并增量 SHA256；成功时文件已关闭且校验通过。
async fn download_binary_streaming_once(
    client: &reqwest::Client,
    url: &str,
    expected_size: u64,
    expected_hex: &str,
    temp_path: &Path,
) -> AppResult<()> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| AppError::context("Download request failed", e))?
        .error_for_status()
        .map_err(|e| AppError::context("Download HTTP error", e))?;

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(temp_path)
        .await
        .map_err(|e| AppError::context("Failed to create temp file", e))?;

    if expected_size == 0 {
        warn!("Asset size is 0; size validation will be skipped");
    }

    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut last_logged_percent = 0u8;
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let chunk: bytes::Bytes =
            chunk_result.map_err(|e| AppError::context("Download stream error", e))?;
        let n = chunk.len() as u64;
        if expected_size > 0 && downloaded + n > expected_size {
            let _ = tokio::fs::remove_file(temp_path).await;
            return Err(AppError::message(format!(
                "Download exceeds expected size ({expected_size} bytes)"
            )));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::context("Failed to write temp file", e))?;
        downloaded += n;

        if expected_size > 0 {
            let percent = ((downloaded as f64 / expected_size as f64) * 100.0) as u8;
            if percent >= last_logged_percent + 10 {
                info!(
                    percent = percent,
                    downloaded = downloaded,
                    total = expected_size,
                    "Download progress"
                );
                last_logged_percent = percent;
            }
        }
    }

    file.shutdown()
        .await
        .map_err(|e| AppError::context("Failed to finalize temp file", e))?;
    drop(file);

    if expected_size > 0 && downloaded != expected_size {
        let _ = tokio::fs::remove_file(temp_path).await;
        return Err(AppError::message(format!(
            "Downloaded file size mismatch: expected {} bytes, got {} bytes",
            expected_size, downloaded
        )));
    }

    let actual_hex = hex::encode(hasher.finalize());
    if actual_hex != expected_hex {
        let _ = tokio::fs::remove_file(temp_path).await;
        return Err(AppError::message(format!(
            "SHA256 mismatch: expected {expected_hex} (from checksum file), got {actual_hex}"
        )));
    }

    info!(
        sha256 = %actual_hex,
        bytes = downloaded,
        "Downloaded binary SHA256 matches release checksum"
    );
    Ok(())
}

async fn download_binary_streaming_retried(
    client: &reqwest::Client,
    url: &str,
    expected_size: u64,
    expected_hex: &str,
    temp_path: &Path,
) -> AppResult<()> {
    let mut last_err: Option<AppError> = None;
    for attempt in 0..DOWNLOAD_MAX_ATTEMPTS {
        if attempt > 0 {
            let _ = tokio::fs::remove_file(temp_path).await;
            sleep(Duration::from_millis(
                DOWNLOAD_RETRY_BASE_MS * (1 << (attempt - 1)),
            ))
            .await;
            warn!(
                attempt = attempt + 1,
                max = DOWNLOAD_MAX_ATTEMPTS,
                "retrying binary download"
            );
        }
        match download_binary_streaming_once(client, url, expected_size, expected_hex, temp_path)
            .await
        {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.expect("binary download retry loop"))
}

async fn fetch_latest_release_uncached(client: &reqwest::Client) -> AppResult<GitHubRelease> {
    let release = client
        .get("https://api.github.com/repos/YUxiangLuo/miao/releases/latest")
        .timeout(Duration::from_secs(60))
        .header("User-Agent", "miao")
        .send()
        .await?
        .error_for_status()
        .map_err(|e| AppError::context("GitHub API returned error", e))?
        .json::<GitHubRelease>()
        .await?;

    Ok(release)
}

async fn fetch_latest_release(
    client: &reqwest::Client,
    state: &Arc<AppState>,
) -> AppResult<GitHubRelease> {
    let cache = state.version_cache.load();
    if let (Some(release), Some(fetched_at)) = (&cache.release, cache.fetched_at) {
        if fetched_at.elapsed() < CACHE_TTL {
            return Ok(release.clone());
        }
    }
    drop(cache);

    let release = fetch_latest_release_uncached(client).await?;
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
            let has_update = release_is_newer_than_current(&current, &latest);
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
        Err(e) => {
            warn!(error = %e, "Failed to fetch latest release from GitHub");
            VersionInfo {
                current,
                latest: None,
                has_update: false,
                download_url: None,
            }
        }
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

fn checksum_asset_name(binary_asset_name: &str) -> String {
    format!("{binary_asset_name}.sha256")
}

fn find_binary_and_checksum_assets<'a>(
    release: &'a GitHubRelease,
    asset_name: &str,
) -> AppResult<(&'a GitHubAsset, &'a GitHubAsset)> {
    let sum_name = checksum_asset_name(asset_name);
    let binary = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| AppError::message("No binary found for current architecture"))?;
    let checksum = release.assets.iter().find(|a| a.name == sum_name).ok_or_else(|| {
        AppError::message(format!(
            "Release is missing checksum asset {sum_name}; upgrade requires a release that publishes .sha256 files"
        ))
    })?;
    Ok((binary, checksum))
}

/// 将 `v1.2.3` / `1.2.3` 解析为 semver；解析失败返回 `None`。
fn parse_semver_tag(tag: &str) -> Option<semver::Version> {
    let s = tag.strip_prefix('v').unwrap_or(tag);
    semver::Version::parse(s).ok()
}

/// 当前运行版本字符串（如 `v0.12.2`）与 Release `tag_name` 比较。
fn release_is_newer_than_current(current: &str, release_tag: &str) -> bool {
    match (parse_semver_tag(current), parse_semver_tag(release_tag)) {
        (Some(c), Some(r)) => r > c,
        (None, _) => {
            error!(
                current = %current,
                "Current version is not valid semver; cannot compare for updates"
            );
            false
        }
        (_, None) => {
            warn!(
                tag = %release_tag,
                "Release tag is not valid semver; treating as no update"
            );
            false
        }
    }
}

/// 对已通过 SHA256 校验的临时文件 chmod 并执行 `--version` 核对。
async fn verify_temp_binary_executable(temp_path: &Path, tag_name: &str) -> AppResult<()> {
    std::fs::set_permissions(temp_path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| AppError::context("Failed to chmod temp binary", e))?;

    let output = tokio::process::Command::new(temp_path)
        .arg("--version")
        .output()
        .await
        .map_err(|e| AppError::context("Failed to run new binary --version", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::message(format!(
            "New binary --version exited with {}: {}",
            output.status,
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout_version_matches_release(&stdout, tag_name) {
        return Err(AppError::message(format!(
            "New binary --version output does not match release {}: {}",
            tag_name,
            stdout.trim()
        )));
    }
    Ok(())
}

fn stdout_version_matches_release(stdout: &str, tag_name: &str) -> bool {
    let lower = stdout.to_ascii_lowercase();
    if !lower.contains("miao") {
        return false;
    }
    let tag_trim = tag_name.trim();
    let no_v = tag_trim.strip_prefix('v').unwrap_or(tag_trim);
    stdout.contains(tag_trim) || stdout.contains(no_v)
}

pub async fn upgrade_binary(state: &Arc<AppState>) -> AppResult<String> {
    if state
        .upgrading
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(AppError::message("Upgrade already in progress"));
    }

    struct UpgradeGuard(Arc<AppState>);
    impl Drop for UpgradeGuard {
        fn drop(&mut self) {
            self.0.upgrading.store(false, Ordering::SeqCst);
        }
    }
    let _guard = UpgradeGuard(state.clone());

    invalidate_release_cache(state).await;
    let release = fetch_latest_release(&state.http_client, state).await?;
    let current = current_version();

    if !release_is_newer_than_current(&current, &release.tag_name) {
        return Ok("Already up to date".to_string());
    }

    let asset_name =
        current_arch_asset_name().ok_or_else(|| AppError::message("Unsupported architecture"))?;
    let (binary_asset, checksum_asset) = find_binary_and_checksum_assets(&release, asset_name)?;

    let expected_hex =
        fetch_checksum_hex_retried(&state.http_client, &checksum_asset.browser_download_url)
            .await?;

    let temp_path = get_temp_binary_path();
    let temp_path = Path::new(&temp_path);

    info!(
        from_version = %current,
        to_version = %release.tag_name,
        binary_url = %binary_asset.browser_download_url,
        size_bytes = binary_asset.size,
        "starting upgrade download"
    );

    download_binary_streaming_retried(
        &state.http_client,
        &binary_asset.browser_download_url,
        binary_asset.size,
        &expected_hex,
        temp_path,
    )
    .await?;

    if let Err(e) = verify_temp_binary_executable(temp_path, &release.tag_name).await {
        let _ = tokio::fs::remove_file(temp_path).await;
        return Err(e);
    }

    let current_exe = std::env::current_exe()?;

    info!("Stopping sing-box before upgrade...");
    stop_sing_internal(state).await;

    let backup_path = format!("{}.bak", current_exe.display());
    fs::rename(&current_exe, &backup_path)
        .map_err(|e| AppError::context("Failed to backup current binary", e))?;

    if let Err(e) = fs::copy(temp_path, &current_exe) {
        let _ = fs::rename(&backup_path, &current_exe);
        let _ = tokio::fs::remove_file(temp_path).await;
        return Err(AppError::context("Failed to install new binary", e));
    }
    if let Err(e) = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755)) {
        let _ = fs::remove_file(&current_exe);
        let _ = fs::rename(&backup_path, &current_exe);
        let _ = tokio::fs::remove_file(temp_path).await;
        return Err(AppError::context(
            "Failed to set permissions on new binary",
            e,
        ));
    }
    let _ = tokio::fs::remove_file(temp_path).await;

    info!(
        from_version = %current,
        to_version = %release.tag_name,
        "upgrade binary installed; restarting process"
    );

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
    use super::{
        current_arch_asset_name, parse_semver_tag, parse_sha256sum_line,
        release_is_newer_than_current, stdout_version_matches_release,
    };

    #[test]
    fn parse_semver_tag_accepts_prefixed_and_unprefixed() {
        assert!(parse_semver_tag("v1.2.3").is_some());
        assert!(parse_semver_tag("1.2.3").is_some());
    }

    #[test]
    fn parse_semver_tag_rejects_invalid() {
        assert!(parse_semver_tag("v1.2").is_none());
        assert!(parse_semver_tag("not-a-version").is_none());
    }

    #[test]
    fn release_is_newer_than_current_semver() {
        assert!(release_is_newer_than_current("v0.9.9", "v0.10.0"));
        assert!(release_is_newer_than_current("v1.2.9", "v1.3.0"));
        assert!(!release_is_newer_than_current("v1.0.0", "v1.0.0"));
        assert!(!release_is_newer_than_current("v2.0.0", "v1.9.9"));
    }

    #[test]
    fn release_is_newer_than_current_pre_release() {
        assert!(release_is_newer_than_current("v1.0.0-beta", "v1.0.0"));
        assert!(!release_is_newer_than_current("v1.0.0", "v1.0.0-beta"));
    }

    #[test]
    fn parse_sha256sum_line_accepts_gnu_format() {
        let line = "abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd  miao-rust-linux-amd64";
        let h = parse_sha256sum_line(line).unwrap();
        assert_eq!(h.len(), 64);
        assert!(h.starts_with("abcdabcd"));
    }

    #[test]
    fn parse_sha256sum_line_accepts_star_filename() {
        let line = "abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd *miao-rust-linux-amd64";
        assert!(parse_sha256sum_line(line).unwrap().starts_with("abcd"));
    }

    #[test]
    fn stdout_version_matches_release_requires_miao_and_tag_or_version() {
        assert!(stdout_version_matches_release("miao v0.12.2\n", "v0.12.2"));
        assert!(stdout_version_matches_release(
            "miao-rust v1.0.0\n",
            "v1.0.0"
        ));
        assert!(!stdout_version_matches_release("other v1.0.0\n", "v1.0.0"));
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
