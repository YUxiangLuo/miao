use std::sync::Arc;
use std::time::Instant;

use std::{fs, path::Path, sync::atomic::Ordering};

use futures::StreamExt;

use sha2::{Digest, Sha256};

use tokio::io::AsyncWriteExt;

use tokio::time::sleep;
use tokio::time::Duration;

use tracing::info;
use tracing::{error, warn};

use crate::error::{AppError, AppResult};

use crate::models::GitHubAsset;
use crate::models::{GitHubRelease, VersionInfo};
use crate::services::singbox::is_sing_box_running;

use crate::services::singbox::{start_sing_internal, stop_sing_internal};
use crate::state::{AppState, VersionCache};
use crate::VERSION;

const CACHE_TTL: Duration = Duration::from_secs(300);

const DOWNLOAD_MAX_ATTEMPTS: u32 = 3;

const DOWNLOAD_RETRY_BASE_MS: u64 = 500;

#[derive(serde::Serialize, serde::Deserialize)]
struct UpgradePending {
    expected_version: String,
    state: String,
    #[serde(default)]
    boot_pid: Option<u32>,
}

fn process_is_alive(pid: u32) -> bool {
    use nix::{errno::Errno, sys::signal, unistd::Pid};

    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    match signal::kill(Pid::from_raw(pid as i32), None) {
        Ok(()) | Err(Errno::EPERM) => true,
        Err(_) => false,
    }
}

fn upgrade_pending_path(current_exe: &Path) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{}.upgrade-pending.json", current_exe.display()))
}

fn write_upgrade_pending(path: &Path, pending: &UpgradePending) -> AppResult<()> {
    let temp = std::path::PathBuf::from(format!("{}.tmp", path.display()));
    let bytes = serde_json::to_vec(pending)?;
    let mut file = fs::File::create(&temp)
        .map_err(|e| AppError::context("Failed to create upgrade marker", e))?;
    std::io::Write::write_all(&mut file, &bytes)
        .map_err(|e| AppError::context("Failed to write upgrade marker", e))?;
    file.sync_all()
        .map_err(|e| AppError::context("Failed to sync upgrade marker", e))?;
    fs::rename(&temp, path).map_err(|e| AppError::context("Failed to activate upgrade marker", e))
}

/// Reconcile a previous self-upgrade before normal startup. The first boot of
/// the new binary moves `installed` to `booting`; a second boot before embedded
/// extraction and the expected data plane become healthy rolls back to the
/// retained executable.
fn upgrade_requires_rollback(current_exe: &Path) -> AppResult<bool> {
    let backup = std::path::PathBuf::from(format!("{}.bak", current_exe.display()));
    let pending_path = upgrade_pending_path(current_exe);
    let bytes = match fs::read(&pending_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // Legacy releases left backups without a marker. Reaching this
            // point proves the current executable can at least start.
            if backup.exists() {
                let _ = fs::remove_file(backup);
            }
            return Ok(false);
        }
        Err(err) => return Err(AppError::context("Failed to read upgrade marker", err)),
    };
    let mut pending: UpgradePending = serde_json::from_slice(&bytes)?;
    if parse_semver_tag(&pending.expected_version) != parse_semver_tag(&current_version()) {
        // The activated executable is not the release recorded by the
        // installer. Do not grant it a health window; restore the retained
        // binary immediately when possible.
        return Ok(backup.exists());
    }
    if pending.state == "installed" {
        pending.state = "booting".to_string();
        pending.boot_pid = Some(std::process::id());
        write_upgrade_pending(&pending_path, &pending)?;
        return Ok(false);
    }
    if pending.state == "booting" && pending.boot_pid.is_some_and(process_is_alive) {
        // Linux CLI allows multiple panel processes. A concurrent launch is
        // not evidence that the first upgraded process crashed.
        return Ok(false);
    }
    Ok(pending.state == "booting" && backup.exists())
}

pub fn reconcile_pending_upgrade() -> AppResult<()> {
    let current_exe = std::env::current_exe()?;
    if !upgrade_requires_rollback(&current_exe)? {
        return Ok(());
    }

    let backup = std::path::PathBuf::from(format!("{}.bak", current_exe.display()));
    let pending_path = upgrade_pending_path(&current_exe);
    let failed = std::path::PathBuf::from(format!("{}.failed", current_exe.display()));
    let _ = fs::remove_file(&failed);
    fs::rename(&current_exe, &failed)
        .map_err(|e| AppError::context("Failed to move unhealthy upgraded binary", e))?;
    if let Err(err) = fs::rename(&backup, &current_exe) {
        let _ = fs::rename(&failed, &current_exe);
        return Err(AppError::context("Failed to restore previous binary", err));
    }
    let _ = fs::remove_file(&pending_path);
    let _ = fs::remove_file(&failed);
    let args: Vec<String> = std::env::args().collect();
    let err = exec_replace(std::process::Command::new(&current_exe).args(&args[1..]));
    Err(AppError::message(format!(
        "Failed to exec restored binary: {err}"
    )))
}

pub fn mark_upgrade_healthy() {
    let Ok(current_exe) = std::env::current_exe() else {
        return;
    };
    mark_upgrade_healthy_at(&current_exe);
}

fn mark_upgrade_healthy_at(current_exe: &Path) {
    let pending = upgrade_pending_path(current_exe);
    let Ok(bytes) = fs::read(&pending) else {
        return;
    };
    let Ok(marker) = serde_json::from_slice::<UpgradePending>(&bytes) else {
        return;
    };
    // A delayed health observer must never acknowledge another boot or an
    // upgrade that this process has just installed but has not exec'd yet.
    if marker.state != "booting"
        || marker.boot_pid != Some(std::process::id())
        || parse_semver_tag(&marker.expected_version) != parse_semver_tag(&current_version())
    {
        return;
    }
    let backup = std::path::PathBuf::from(format!("{}.bak", current_exe.display()));
    let _ = fs::remove_file(backup);
    let _ = fs::remove_file(pending);
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
            "SHA256 mismatch: expected {expected_hex} (from GitHub release metadata), got {actual_hex}"
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
    // Skip GitHub while the kernel is down (no TUN / likely no route).
    if !is_sing_box_running(state).await {
        return version_info_without_release(current);
    }

    match fetch_latest_release(&state.http_client, state).await {
        Ok(release) => version_info_from_release(current, &release),
        Err(e) => {
            warn!(error = %e, "Failed to fetch latest release from GitHub");
            version_info_without_release(current)
        }
    }
}

fn version_info_from_release(current: String, release: &GitHubRelease) -> VersionInfo {
    let latest = release.tag_name.clone();
    let has_update = release_is_newer_than_current(&current, &latest);
    let download_url = {
        current_arch_asset_name().and_then(|asset_name| {
            release
                .assets
                .iter()
                .find(|asset| asset.name == asset_name)
                .map(|asset| asset.browser_download_url.clone())
        })
    };

    VersionInfo {
        current,
        latest: Some(latest),
        has_update,
        download_url,
        upgrade_supported: true,
    }
}

fn get_temp_binary_path() -> String {
    let pid = std::process::id();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::env::temp_dir()
        .join(format!("miao-new-{pid}-{timestamp}"))
        .to_string_lossy()
        .into_owned()
}

fn binary_asset_and_digest<'a>(
    release: &'a GitHubRelease,
    asset_name: &str,
) -> AppResult<(&'a GitHubAsset, String)> {
    let binary = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| AppError::message("No binary found for current architecture"))?;
    let hex = binary
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| {
            AppError::message("Release asset is missing a valid GitHub SHA256 digest")
        })?;
    Ok((binary, hex.to_ascii_lowercase()))
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
    set_executable(temp_path).map_err(|e| AppError::context("Failed to chmod temp binary", e))?;

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
    let guard = UpgradeGuard(state.clone());

    invalidate_release_cache(state).await;
    let release = fetch_latest_release(&state.http_client, state).await?;
    let current = current_version();

    if !release_is_newer_than_current(&current, &release.tag_name) {
        return Ok("Already up to date".to_string());
    }

    let asset_name =
        current_arch_asset_name().ok_or_else(|| AppError::message("Unsupported architecture"))?;
    let (binary_asset, expected_hex) = binary_asset_and_digest(&release, asset_name)?;

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
    let staged_path = std::path::PathBuf::from(format!("{}.new", current_exe.display()));
    let _ = fs::remove_file(&staged_path);
    fs::copy(temp_path, &staged_path)
        .map_err(|e| AppError::context("Failed to stage new binary", e))?;
    set_executable(&staged_path)
        .map_err(|e| AppError::context("Failed to set staged binary permissions", e))?;
    fs::File::open(&staged_path)
        .and_then(|file| file.sync_all())
        .map_err(|e| AppError::context("Failed to sync staged binary", e))?;
    let _ = tokio::fs::remove_file(temp_path).await;

    // Close admission and drain all OTHER accepted writes before taking the
    // transaction lock. The shared request lease survives into delayed exec,
    // so shutdown still waits for this handoff without making us await ourselves.
    let quiescence = match state.quiesce_for_upgrade().await {
        Ok(quiescence) => quiescence,
        Err(err) => {
            let _ = fs::remove_file(&staged_path);
            return Err(err);
        }
    };
    let lifecycle_guard = state.config_update.clone().lock_owned().await;
    if state.lifecycle.is_shutting_down() {
        let _ = fs::remove_file(&staged_path);
        return Err(AppError::message("Upgrade cancelled by shutdown"));
    }
    state.next_sub_refresh();
    let was_running = is_sing_box_running(state).await;

    info!("Stopping sing-box before upgrade...");
    stop_sing_internal(state).await;

    let backup_path = format!("{}.bak", current_exe.display());
    let backup = Path::new(&backup_path);
    let install_result = (|| -> AppResult<()> {
        if backup.exists() {
            return Err(AppError::message(
                "A previous upgrade backup is still pending; restart miao before upgrading again",
            ));
        }
        fs::rename(&current_exe, backup)
            .map_err(|e| AppError::context("Failed to backup current binary", e))?;
        if let Err(err) = fs::rename(&staged_path, &current_exe) {
            let _ = fs::rename(backup, &current_exe);
            return Err(AppError::context("Failed to activate new binary", err));
        }
        if let Err(err) = write_upgrade_pending(
            &upgrade_pending_path(&current_exe),
            &UpgradePending {
                expected_version: release.tag_name.clone(),
                state: "installed".to_string(),
                boot_pid: None,
            },
        ) {
            let _ = fs::remove_file(&current_exe);
            let _ = fs::rename(backup, &current_exe);
            return Err(err);
        }
        Ok(())
    })();
    if let Err(install_err) = install_result {
        let _ = fs::remove_file(&staged_path);
        if was_running {
            if let Err(restart_err) = start_sing_internal(state).await {
                return Err(AppError::message(format!(
                    "{}. Previous sing-box restart failed: {}",
                    install_err, restart_err
                )));
            }
        }
        return Err(install_err);
    }

    info!(
        from_version = %current,
        to_version = %release.tag_name,
        "upgrade binary installed; restarting process"
    );

    let new_version = release.tag_name.clone();
    tokio::spawn(async move {
        let state = guard.0.clone();
        let _guard = guard;
        let _quiescence = quiescence;
        let _lifecycle_guard = lifecycle_guard;
        sleep(Duration::from_millis(500)).await;

        // 内嵌文件的刷新由新进程启动时的 extract_sing_box 无条件重释放保证,此处无需清理

        let args: Vec<String> = std::env::args().collect();
        let result = state.lifecycle.with_upgrade_handoff(|| {
            exec_replace(std::process::Command::new(&current_exe).args(&args[1..]))
        });
        let Some(err) = result else {
            // Shutdown won after installation but before exec. Do not replace
            // its process out from under it; restore the on-disk old executable.
            match fs::rename(&backup_path, &current_exe) {
                Ok(()) => {
                    let _ = fs::remove_file(upgrade_pending_path(&current_exe));
                }
                Err(err) => error!(%err, "Failed to restore cancelled upgrade backup"),
            }
            info!("Upgrade handoff cancelled by shutdown");
            return;
        };

        error!("Failed to exec new binary: {}", err);
        error!("Attempting to restore from backup...");

        if fs::rename(&backup_path, &current_exe).is_ok() {
            let _ = fs::remove_file(upgrade_pending_path(&current_exe));
            let _ = set_executable(&current_exe);
            error!("Restored from backup, restarting with old version...");
            if state
                .lifecycle
                .with_upgrade_handoff(|| {
                    exec_replace(std::process::Command::new(&current_exe).args(&args[1..]))
                })
                .is_none()
            {
                info!("Restored upgrade backup; shutdown cancelled fallback exec");
                return;
            }
        }
        let diag = format!(
            "miao upgrade failure: exec and backup restore both failed.\nbinary: {:?}\nbackup: {}\n",
            current_exe, backup_path
        );
        let _ = std::fs::write(upgrade_failure_log_path(), &diag);
        error!(
            "Diagnostics written to {}",
            upgrade_failure_log_path().display()
        );
        error!("Failed to restore from backup, manual intervention required");
        std::process::exit(1);
    });

    Ok(new_version)
}

fn set_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

fn exec_replace(command: &mut std::process::Command) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    command.exec()
}

fn upgrade_failure_log_path() -> std::path::PathBuf {
    std::path::PathBuf::from("/tmp/miao-upgrade-failure.log")
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

fn version_info_without_release(current: String) -> VersionInfo {
    VersionInfo {
        current,
        latest: None,
        has_update: false,
        download_url: None,
        upgrade_supported: true,
    }
}

#[cfg(test)]
mod tests;
