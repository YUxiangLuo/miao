use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{atomic::Ordering, Arc},
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::read::GzDecoder;
use futures::StreamExt;
use nix::{
    sys::{
        statfs::{statfs, TMPFS_MAGIC},
        statvfs::{statvfs, FsFlags},
    },
    unistd::{chown, Gid, Uid},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::mpsc::UnboundedSender,
    time::Duration,
};
use tracing::{info, warn};

use crate::{
    error::{AppError, AppResult},
    models::{AgentConfigRequest, AgentProviderInfo, AgentStatusData},
    state::AppState,
};

pub const PI_VERSION: &str = "0.84.1";
pub const REQUIRED_SPACE_BYTES: u64 = 256 * 1024 * 1024;
const MIN_AVAILABLE_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
const MIN_RUNTIME_FREE_BYTES: u64 = 16 * 1024 * 1024;
const MIN_AVAILABLE_INODES: u64 = 64;
const INSTALL_ROOT: &str = "/tmp/miao-pi-agent";
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const SANDBOX_ID_MIN: u32 = 60_000;
const SANDBOX_ID_COUNT: u32 = 5_000;
const NOBODY_ID: u32 = 65_534;

const PROVIDERS: &[(&str, &str)] = &[
    ("anthropic", "Anthropic"),
    ("openai", "OpenAI"),
    ("google", "Google Gemini"),
    ("openrouter", "OpenRouter"),
    ("deepseek", "DeepSeek"),
    ("xai", "xAI"),
    ("groq", "Groq"),
    ("mistral", "Mistral"),
    ("kimi-coding", "Kimi For Coding"),
    ("zai", "ZAI Coding Plan"),
];

const SYSTEM_PROMPT: &str = r#"You are the built-in assistant for Miao, a sing-box based transparent proxy manager.
Reply in the same language as the user. Keep answers practical and concise.
You have no system tools and cannot inspect or modify the host. Never claim that you ran a command or changed configuration.
Never ask the user to paste subscription URLs, proxy passwords, API keys, tokens, or other secrets into chat.
When troubleshooting, explain safe checks the user can perform in Miao and clearly distinguish facts from suggestions."#;

#[derive(Clone, Copy)]
struct PiAsset {
    archive_name: &'static str,
    archive_size: u64,
    binary_size: u64,
    sha256: &'static str,
    loader_path: &'static str,
}

// Pinned official release metadata. A Pi upgrade must update the archive size,
// extracted binary size, and SHA-256 together after independent verification.
#[cfg(target_arch = "x86_64")]
const PI_ASSET: PiAsset = PiAsset {
    archive_name: "pi-linux-x64.tar.gz",
    archive_size: 43_326_675,
    binary_size: 106_711_168,
    sha256: "5634d7ebd18274b63af3371e942f342d74bea012389575c1d1ff15ce6ca80c2f",
    loader_path: "/lib64/ld-linux-x86-64.so.2",
};

#[cfg(target_arch = "aarch64")]
const PI_ASSET: PiAsset = PiAsset {
    archive_name: "pi-linux-arm64.tar.gz",
    archive_size: 43_363_297,
    binary_size: 106_735_760,
    sha256: "ab95c058a4651b5ff5d8c878e524edfb776263c7a444f325505f247c056eecfc",
    loader_path: "/lib/ld-linux-aarch64.so.1",
};

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
compile_error!("The Pi agent MVP supports only x86_64 and aarch64");

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StoredAgentConfig {
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub api_key: String,
}

#[derive(Clone, Debug)]
pub enum AgentPreparationEvent {
    Checking,
    Downloading { total_bytes: u64 },
    Verifying,
    Extracting,
}

pub struct PiProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
    pub runtime_dir: PathBuf,
}

fn cleanup_runtime_path(runtime_dir: &Path) {
    let _ = fs::remove_dir_all(runtime_dir);
    if let Some(runtime_root) = runtime_dir.parent() {
        let _ = fs::remove_dir(runtime_root);
    }
}

impl Drop for PiProcess {
    fn drop(&mut self) {
        // WebSocket upgrade tasks may be dropped directly during server shutdown.
        // kill_on_drop handles the child; this guard also removes its private files.
        cleanup_runtime_path(&self.runtime_dir);
    }
}

fn provider_infos() -> Vec<AgentProviderInfo> {
    PROVIDERS
        .iter()
        .map(|(id, label)| AgentProviderInfo { id, label })
        .collect()
}

fn provider_is_supported(provider: &str) -> bool {
    PROVIDERS.iter().any(|(id, _)| *id == provider)
}

fn provider_api_key_env(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        "google" => Some("GEMINI_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "deepseek" => Some("DEEPSEEK_API_KEY"),
        "xai" => Some("XAI_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "mistral" => Some("MISTRAL_API_KEY"),
        "kimi-coding" => Some("KIMI_API_KEY"),
        "zai" => Some("ZAI_API_KEY"),
        _ => None,
    }
}

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

fn install_root() -> PathBuf {
    PathBuf::from(INSTALL_ROOT)
}

fn version_dir_at(root: &Path) -> PathBuf {
    root.join(format!("v{PI_VERSION}"))
}

fn file_is_safe(path: &Path, expected_size: Option<u64>) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.file_type().is_file()
        || metadata.uid() != Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o022 != 0
    {
        return false;
    }
    expected_size.is_none_or(|size| metadata.len() == size)
}

fn pi_is_installed_at(root: &Path) -> bool {
    let version_dir = version_dir_at(root);
    file_is_safe(&version_dir.join("pi"), Some(PI_ASSET.binary_size))
        && file_is_safe(&version_dir.join("theme/dark.json"), None)
        && file_is_safe(&version_dir.join("theme/light.json"), None)
        && file_is_safe(&version_dir.join("theme/theme-schema.json"), None)
}

pub fn pi_is_installed() -> bool {
    pi_is_installed_at(&install_root())
}

fn ensure_secure_dir(path: &Path, mode: u32) -> AppResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(AppError::message(format!(
                    "Unsafe agent path is not a directory: {}",
                    path.display()
                )));
            }
            if metadata.uid() != Uid::effective().as_raw() {
                return Err(AppError::message(format!(
                    "Agent directory has an unexpected owner: {}",
                    path.display()
                )));
            }
            if metadata.permissions().mode() & 0o022 != 0 {
                return Err(AppError::message(format!(
                    "Agent directory is writable by another user: {}",
                    path.display()
                )));
            }
            if metadata.permissions().mode() & 0o777 != mode {
                fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            DirBuilder::new().mode(mode).create(path)?;
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

fn available_space_bytes(path: &Path) -> AppResult<u64> {
    let stats = statvfs(path)
        .map_err(|err| AppError::message(format!("Failed to inspect /tmp space: {err}")))?;
    Ok((stats.blocks_available() as u64).saturating_mul(stats.fragment_size() as u64))
}

fn available_inode_count(path: &Path) -> AppResult<u64> {
    let stats = statvfs(path)
        .map_err(|err| AppError::message(format!("Failed to inspect /tmp inodes: {err}")))?;
    Ok(stats.files_available() as u64)
}

fn path_is_noexec(path: &Path) -> AppResult<bool> {
    let stats = statvfs(path)
        .map_err(|err| AppError::message(format!("Failed to inspect /tmp flags: {err}")))?;
    Ok(stats.flags().contains(FsFlags::ST_NOEXEC))
}

fn path_is_tmpfs(path: &Path) -> bool {
    statfs(path)
        .map(|stats| stats.filesystem_type() == TMPFS_MAGIC)
        .unwrap_or(false)
}

fn parse_mem_available(content: &str) -> Option<u64> {
    content.lines().find_map(|line| {
        let rest = line.strip_prefix("MemAvailable:")?;
        let kib = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        Some(kib.saturating_mul(1024))
    })
}

fn available_memory_bytes() -> Option<u64> {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|content| parse_mem_available(&content))
}

fn capability_reason(
    installed: bool,
    loader_exists: bool,
    noexec: bool,
    available_space: u64,
    available_inodes: u64,
    tmpfs: bool,
    available_memory: Option<u64>,
) -> Option<String> {
    if !loader_exists {
        return Some("当前系统缺少 glibc，Pi Agent MVP 暂不支持原生 OpenWrt/musl".to_string());
    }
    if noexec {
        return Some("/tmp 以 noexec 挂载，无法启动 Pi Agent".to_string());
    }
    let required = if installed {
        MIN_RUNTIME_FREE_BYTES
    } else {
        REQUIRED_SPACE_BYTES
    };
    if available_space < required {
        return Some(format!(
            "/tmp 空间不足，需要至少 {} MiB 可用空间",
            required / 1024 / 1024
        ));
    }
    if available_inodes < MIN_AVAILABLE_INODES {
        return Some(format!(
            "/tmp 可用 inode 不足，需要至少 {MIN_AVAILABLE_INODES} 个"
        ));
    }
    match available_memory {
        Some(bytes) if bytes < MIN_AVAILABLE_MEMORY_BYTES && tmpfs => {
            Some("/tmp 位于内存文件系统，当前可用内存不足 512 MiB".to_string())
        }
        Some(bytes) if bytes < MIN_AVAILABLE_MEMORY_BYTES => {
            Some("当前可用内存不足 512 MiB，无法安全启动 Pi Agent".to_string())
        }
        Some(_) => None,
        None => Some("无法检查当前可用内存".to_string()),
    }
}

fn agent_data_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".miao-agent")
}

fn stored_config_path(config_path: &Path) -> PathBuf {
    agent_data_dir(config_path).join("credentials.json")
}

pub async fn load_agent_config(config_path: &Path) -> AppResult<Option<StoredAgentConfig>> {
    let path = stored_config_path(config_path);
    let metadata = match tokio::fs::symlink_metadata(&path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AppError::context(
                "Failed to inspect stored agent credentials",
                err,
            ))
        }
    };
    if !metadata.file_type().is_file() || metadata.uid() != Uid::effective().as_raw() {
        return Err(AppError::message("Stored agent credential path is unsafe"));
    }
    if metadata.permissions().mode() & 0o777 != 0o600 {
        tokio::fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .await
            .map_err(|err| {
                AppError::context("Failed to repair stored agent credential permissions", err)
            })?;
    }

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|err| AppError::context("Failed to read stored agent credentials", err))?;
    let stored: StoredAgentConfig = serde_json::from_slice(&bytes)
        .map_err(|err| AppError::context("Invalid stored agent credentials", err))?;
    normalized_config(AgentConfigRequest {
        provider: stored.provider,
        model: stored.model,
        api_key: stored.api_key,
    })
    .map(Some)
}

fn normalized_config(request: AgentConfigRequest) -> AppResult<StoredAgentConfig> {
    let provider = request.provider.trim().to_ascii_lowercase();
    if !provider_is_supported(&provider) {
        return Err(AppError::message("Unsupported AI provider"));
    }

    let api_key = request.api_key.trim().to_string();
    if api_key.is_empty() || api_key.len() > 4096 || api_key.chars().any(char::is_control) {
        return Err(AppError::message("API key is empty or invalid"));
    }

    let model = request.model.and_then(|model| {
        let model = model.trim().to_string();
        (!model.is_empty()).then_some(model)
    });
    if model
        .as_ref()
        .is_some_and(|model| model.len() > 256 || model.chars().any(char::is_control))
    {
        return Err(AppError::message("Model ID is invalid"));
    }

    Ok(StoredAgentConfig {
        provider,
        model,
        api_key,
    })
}

fn write_stored_config(config_path: &Path, config: &StoredAgentConfig) -> AppResult<()> {
    let data_dir = agent_data_dir(config_path);
    ensure_secure_dir(&data_dir, 0o700)?;

    let target = stored_config_path(config_path);
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        if !metadata.file_type().is_file() || metadata.uid() != Uid::effective().as_raw() {
            return Err(AppError::message("Stored agent credential path is unsafe"));
        }
    }

    let temp = data_dir.join(format!("credentials.{}.tmp", unique_suffix()));
    let bytes = serde_json::to_vec(config)?;
    let result = (|| -> AppResult<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, &target)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub async fn save_agent_config(
    config_path: &Path,
    request: AgentConfigRequest,
) -> AppResult<StoredAgentConfig> {
    let config = normalized_config(request)?;
    let path = config_path.to_path_buf();
    let config_for_write = config.clone();
    tokio::task::spawn_blocking(move || write_stored_config(&path, &config_for_write))
        .await
        .map_err(|err| AppError::message(format!("Agent credential writer failed: {err}")))??;
    Ok(config)
}

struct AgentEnvironment {
    installed: bool,
    available_space: Option<u64>,
    available_inodes: Option<u64>,
    available_memory: Option<u64>,
    reason: Option<String>,
}

fn inspect_agent_environment() -> AgentEnvironment {
    let installed = pi_is_installed();
    let available_space = available_space_bytes(Path::new("/tmp")).ok();
    let available_inodes = available_inode_count(Path::new("/tmp")).ok();
    let available_memory = available_memory_bytes();
    let reason = match (available_space, available_inodes) {
        (Some(space), Some(inodes)) => capability_reason(
            installed,
            Path::new(PI_ASSET.loader_path).exists(),
            path_is_noexec(Path::new("/tmp")).unwrap_or(true),
            space,
            inodes,
            path_is_tmpfs(Path::new("/tmp")),
            available_memory,
        ),
        (None, _) => Some("无法检查 /tmp 可用空间".to_string()),
        (_, None) => Some("无法检查 /tmp 可用 inode".to_string()),
    };
    AgentEnvironment {
        installed,
        available_space,
        available_inodes,
        available_memory,
        reason,
    }
}

pub fn agent_unsupported_reason() -> Option<String> {
    inspect_agent_environment().reason
}

pub async fn agent_status(state: &Arc<AppState>) -> AppResult<AgentStatusData> {
    let environment = inspect_agent_environment();
    let config = load_agent_config(&state.config_path).await?;

    Ok(AgentStatusData {
        supported: environment.reason.is_none(),
        reason: environment.reason,
        installed: environment.installed,
        configured: config.is_some(),
        session_active: state.agent_session_active.load(Ordering::Relaxed),
        version: PI_VERSION,
        provider: config.as_ref().map(|config| config.provider.clone()),
        model: config.and_then(|config| config.model),
        providers: provider_infos(),
        required_space_bytes: if environment.installed {
            MIN_RUNTIME_FREE_BYTES
        } else {
            REQUIRED_SPACE_BYTES
        },
        available_space_bytes: environment.available_space,
        required_tmp_inodes: MIN_AVAILABLE_INODES,
        available_tmp_inodes: environment.available_inodes,
        required_memory_bytes: MIN_AVAILABLE_MEMORY_BYTES,
        available_memory_bytes: environment.available_memory,
    })
}

fn asset_url() -> String {
    format!(
        "https://github.com/earendil-works/pi/releases/download/v{PI_VERSION}/{}",
        PI_ASSET.archive_name
    )
}

async fn download_archive_once(client: &reqwest::Client, destination: &Path) -> AppResult<()> {
    let response = client
        .get(asset_url())
        .timeout(DOWNLOAD_TIMEOUT)
        .header("User-Agent", "miao")
        .send()
        .await
        .map_err(|err| AppError::context("Failed to download Pi Agent", err))?
        .error_for_status()
        .map_err(|err| AppError::context("Pi Agent download returned an error", err))?;

    if response
        .content_length()
        .is_some_and(|length| length != PI_ASSET.archive_size)
    {
        return Err(AppError::message(
            "Pi Agent download size changed unexpectedly",
        ));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .await?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| AppError::context("Pi Agent download failed", err))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > PI_ASSET.archive_size {
            return Err(AppError::message(
                "Pi Agent download exceeded the expected size",
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    file.sync_all().await?;

    if downloaded != PI_ASSET.archive_size {
        return Err(AppError::message(format!(
            "Pi Agent download is incomplete: expected {} bytes, received {downloaded}",
            PI_ASSET.archive_size
        )));
    }
    let actual = hex::encode(hasher.finalize());
    if actual != PI_ASSET.sha256 {
        return Err(AppError::message("Pi Agent SHA-256 verification failed"));
    }
    Ok(())
}

async fn download_archive(client: &reqwest::Client, destination: &Path) -> AppResult<()> {
    let mut last_error = None;
    for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
        let _ = tokio::fs::remove_file(destination).await;
        match download_archive_once(client, destination).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = Some(err);
                if attempt + 1 < MAX_DOWNLOAD_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| AppError::message("Pi Agent download failed")))
}

struct TemporaryInstallArtifacts {
    archive: PathBuf,
    staging: PathBuf,
}

impl Drop for TemporaryInstallArtifacts {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.archive);
        let _ = fs::remove_dir_all(&self.staging);
    }
}

fn extract_minimal_archive(archive_path: &Path, destination: &Path) -> AppResult<()> {
    let file = fs::File::open(archive_path)?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let wanted = [
        "pi/pi",
        "pi/theme/dark.json",
        "pi/theme/light.json",
        "pi/theme/theme-schema.json",
    ];
    let mut extracted = BTreeMap::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let Some(path_text) = path.to_str() else {
            continue;
        };
        if !wanted.contains(&path_text) {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(AppError::message(
                "Pi archive contains an invalid entry type",
            ));
        }
        let relative = path
            .strip_prefix("pi")
            .map_err(|_| AppError::message("Pi archive path is invalid"))?;
        let output = destination.join(relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&output)?;
        extracted.insert(path_text.to_string(), ());
    }

    if wanted.iter().any(|path| !extracted.contains_key(*path)) {
        return Err(AppError::message(
            "Pi archive is missing required runtime files",
        ));
    }
    if fs::metadata(destination.join("pi"))?.len() != PI_ASSET.binary_size {
        return Err(AppError::message("Extracted Pi binary size is invalid"));
    }

    fs::set_permissions(destination, fs::Permissions::from_mode(0o755))?;
    fs::set_permissions(destination.join("pi"), fs::Permissions::from_mode(0o555))?;
    fs::set_permissions(destination.join("theme"), fs::Permissions::from_mode(0o755))?;
    for theme in ["dark.json", "light.json", "theme-schema.json"] {
        fs::set_permissions(
            destination.join("theme").join(theme),
            fs::Permissions::from_mode(0o444),
        )?;
    }
    Ok(())
}

pub async fn ensure_pi_installed(
    state: &Arc<AppState>,
    events: &UnboundedSender<AgentPreparationEvent>,
) -> AppResult<PathBuf> {
    let _install_guard = state.agent_install.lock().await;
    let root = install_root();
    ensure_secure_dir(&root, 0o755)?;
    let _ = events.send(AgentPreparationEvent::Checking);

    if pi_is_installed_at(&root) {
        return Ok(version_dir_at(&root).join("pi"));
    }

    let space = available_space_bytes(Path::new("/tmp"))?;
    if let Some(reason) = capability_reason(
        false,
        Path::new(PI_ASSET.loader_path).exists(),
        path_is_noexec(Path::new("/tmp"))?,
        space,
        available_inode_count(Path::new("/tmp"))?,
        path_is_tmpfs(Path::new("/tmp")),
        available_memory_bytes(),
    ) {
        return Err(AppError::message(reason));
    }

    let archive_path = root.join(format!("download-{}.tar.gz", unique_suffix()));
    let staging = root.join(format!("staging-{}", unique_suffix()));
    DirBuilder::new().mode(0o700).create(&staging)?;
    let _artifacts = TemporaryInstallArtifacts {
        archive: archive_path.clone(),
        staging: staging.clone(),
    };

    let result = async {
        let _ = events.send(AgentPreparationEvent::Downloading {
            total_bytes: PI_ASSET.archive_size,
        });
        download_archive(&state.http_client, &archive_path).await?;
        let _ = events.send(AgentPreparationEvent::Verifying);
        let _ = events.send(AgentPreparationEvent::Extracting);

        let archive_for_extract = archive_path.clone();
        let staging_for_extract = staging.clone();
        tokio::task::spawn_blocking(move || {
            extract_minimal_archive(&archive_for_extract, &staging_for_extract)
        })
        .await
        .map_err(|err| AppError::message(format!("Pi extractor failed: {err}")))??;

        let target = version_dir_at(&root);
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::rename(&staging, &target)?;
        info!(version = PI_VERSION, path = ?target, "Pi Agent installed lazily");
        Ok(target.join("pi"))
    }
    .await;

    result
}

fn write_runtime_files(runtime_dir: &Path, config: &StoredAgentConfig) -> AppResult<()> {
    let config_dir = runtime_dir.join("config");
    let work_dir = runtime_dir.join("work");
    let tmp_dir = runtime_dir.join("tmp");
    for path in [&config_dir, &work_dir, &tmp_dir] {
        DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    }

    let settings = serde_json::json!({
        "defaultProvider": config.provider,
        "defaultModel": config.model,
        "defaultProjectTrust": "never",
        "enableInstallTelemetry": false,
        "quietStartup": true
    });

    let path = config_dir.join("settings.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&serde_json::to_vec(&settings)?)?;
    file.sync_all()?;
    Ok(())
}

fn id_is_declared(id: u32) -> bool {
    [("/etc/passwd", 2_usize), ("/etc/group", 2_usize)]
        .into_iter()
        .any(|(path, field)| {
            fs::read_to_string(path).is_ok_and(|content| {
                content.lines().any(|line| {
                    line.split(':')
                        .nth(field)
                        .and_then(|value| value.parse::<u32>().ok())
                        == Some(id)
                })
            })
        })
}

fn sandbox_id() -> u32 {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        .wrapping_add(std::process::id());
    for offset in 0..SANDBOX_ID_COUNT {
        let id = SANDBOX_ID_MIN + (seed.wrapping_add(offset) % SANDBOX_ID_COUNT);
        if !id_is_declared(id) {
            return id;
        }
    }
    NOBODY_ID
}

fn chown_runtime_for_unprivileged_user(runtime_dir: &Path) -> AppResult<Option<u32>> {
    if !Uid::effective().is_root() {
        return Ok(None);
    }

    let preferred_id = sandbox_id();
    for id in [preferred_id, NOBODY_ID] {
        let uid = Uid::from_raw(id);
        let gid = Gid::from_raw(id);
        let result = [
            runtime_dir.to_path_buf(),
            runtime_dir.join("config"),
            runtime_dir.join("config/settings.json"),
            runtime_dir.join("work"),
            runtime_dir.join("tmp"),
        ]
        .into_iter()
        .try_for_each(|path| chown(&path, Some(uid), Some(gid)));
        if result.is_ok() {
            return Ok(Some(id));
        }
    }

    Err(AppError::message(
        "Failed to assign an unprivileged Pi runtime identity",
    ))
}

fn cleanup_stale_runtime_dirs(runtime_root: &Path) {
    let Ok(entries) = fs::read_dir(runtime_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                let _ = fs::remove_dir_all(path);
            }
            Ok(_) => {
                let _ = fs::remove_file(path);
            }
            Err(_) => {}
        }
    }
}

pub async fn spawn_pi_process(binary: &Path, config: &StoredAgentConfig) -> AppResult<PiProcess> {
    let api_key_env = provider_api_key_env(&config.provider)
        .ok_or_else(|| AppError::message("Unsupported stored AI provider"))?;
    let root = install_root();
    let runtime_root = root.join(format!("runtime-{}", std::process::id()));
    ensure_secure_dir(&runtime_root, 0o711)?;
    cleanup_stale_runtime_dirs(&runtime_root);
    let runtime_dir = runtime_root.join(unique_suffix());
    DirBuilder::new().mode(0o700).create(&runtime_dir)?;

    let sandbox_id = match write_runtime_files(&runtime_dir, config)
        .and_then(|_| chown_runtime_for_unprivileged_user(&runtime_dir))
    {
        Ok(id) => id,
        Err(err) => {
            cleanup_runtime_path(&runtime_dir);
            return Err(err);
        }
    };

    let config_dir = runtime_dir.join("config");
    let work_dir = runtime_dir.join("work");
    let tmp_dir = runtime_dir.join("tmp");
    let mut command = Command::new(binary);
    command
        .arg("--mode")
        .arg("rpc")
        .arg("--no-session")
        .arg("--no-tools")
        .arg("--no-extensions")
        .arg("--no-skills")
        .arg("--no-prompt-templates")
        .arg("--no-themes")
        .arg("--no-context-files")
        .arg("--no-approve")
        .arg("--offline")
        .arg("--provider")
        .arg(&config.provider)
        .arg("--system-prompt")
        .arg(SYSTEM_PROMPT);
    if let Some(model) = &config.model {
        command.arg("--model").arg(model);
    }

    command
        .current_dir(&work_dir)
        .env_clear()
        .env("HOME", &runtime_dir)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("TMPDIR", &tmp_dir)
        .env("PI_CODING_AGENT_DIR", &config_dir)
        .env("PI_OFFLINE", "1")
        .env("PI_SKIP_VERSION_CHECK", "1")
        .env("PI_TELEMETRY", "0")
        .env(api_key_env, &config.api_key)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
    ] {
        if let Ok(value) = std::env::var(key) {
            command.env(key, value);
        }
    }
    if let Some(id) = sandbox_id {
        command.gid(id).uid(id);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            cleanup_runtime_path(&runtime_dir);
            return Err(AppError::context("Failed to start Pi Agent", err));
        }
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.start_kill();
        let _ = child.wait().await;
        cleanup_runtime_path(&runtime_dir);
        return Err(AppError::message("Pi Agent RPC pipes are unavailable"));
    };

    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut stderr = stderr;
            let mut buffer = [0_u8; 4096];
            let mut warned = false;
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(_) if !warned => {
                        // Do not log provider output: it can contain prompts or credentials.
                        warn!("Pi Agent emitted diagnostic output on stderr");
                        warned = true;
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
    }

    Ok(PiProcess {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        runtime_dir,
    })
}

pub async fn stop_pi_process(process: &mut PiProcess) {
    let _ = process.stdin.write_all(b"{\"type\":\"abort\"}\n").await;
    let _ = process.stdin.flush().await;

    let _ = process.child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(1), process.child.wait()).await;
    let _ = tokio::fs::remove_dir_all(&process.runtime_dir).await;
    if let Some(runtime_root) = process.runtime_dir.parent() {
        let _ = tokio::fs::remove_dir(runtime_root).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        capability_reason, load_agent_config, normalized_config, parse_mem_available,
        pi_is_installed_at, write_runtime_files, write_stored_config, AgentConfigRequest, PI_ASSET,
        REQUIRED_SPACE_BYTES,
    };
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn memory_parser_reads_mem_available_in_bytes() {
        assert_eq!(
            parse_mem_available("MemTotal: 10 kB\nMemAvailable: 12345 kB\n"),
            Some(12_641_280)
        );
    }

    #[test]
    fn capability_rejects_missing_glibc_noexec_and_low_resources() {
        assert!(capability_reason(
            false,
            false,
            false,
            u64::MAX,
            u64::MAX,
            false,
            Some(u64::MAX),
        )
        .unwrap()
        .contains("glibc"));
        assert!(
            capability_reason(false, true, true, u64::MAX, u64::MAX, false, Some(u64::MAX),)
                .unwrap()
                .contains("noexec")
        );
        assert!(capability_reason(
            false,
            true,
            false,
            REQUIRED_SPACE_BYTES - 1,
            u64::MAX,
            false,
            Some(u64::MAX),
        )
        .unwrap()
        .contains("空间不足"));
        assert!(
            capability_reason(false, true, false, u64::MAX, 1, false, Some(u64::MAX),)
                .unwrap()
                .contains("inode")
        );
        assert!(
            capability_reason(false, true, false, u64::MAX, u64::MAX, true, Some(1),)
                .unwrap()
                .contains("内存不足")
        );
        assert!(
            capability_reason(false, true, false, u64::MAX, u64::MAX, false, Some(1),)
                .unwrap()
                .contains("内存不足")
        );
        assert!(capability_reason(
            false,
            true,
            false,
            u64::MAX,
            u64::MAX,
            false,
            Some(u64::MAX),
        )
        .is_none());
    }

    #[test]
    fn config_validation_rejects_unknown_provider_and_control_characters() {
        let unknown = normalized_config(AgentConfigRequest {
            provider: "unknown".to_string(),
            model: None,
            api_key: "secret".to_string(),
        });
        assert!(unknown.is_err());

        let invalid_key = normalized_config(AgentConfigRequest {
            provider: "openai".to_string(),
            model: None,
            api_key: "bad\nkey".to_string(),
        });
        assert!(invalid_key.is_err());
    }

    #[tokio::test]
    async fn publicly_readable_stored_credentials_are_repaired_before_reading() {
        let root = std::env::temp_dir().join(format!(
            "miao-agent-permissions-test-{}",
            super::unique_suffix()
        ));
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("config.yaml");
        let config = super::StoredAgentConfig {
            provider: "openai".to_string(),
            model: None,
            api_key: "secret-key".to_string(),
        };
        write_stored_config(&config_path, &config).unwrap();
        fs::set_permissions(
            root.join(".miao-agent/credentials.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        assert_eq!(load_agent_config(&config_path).await.unwrap(), Some(config));
        assert_eq!(
            fs::metadata(root.join(".miao-agent/credentials.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stored_credentials_are_private_and_never_mark_an_incomplete_install_valid() {
        let root = std::env::temp_dir().join(format!(
            "miao-agent-service-test-{}",
            super::unique_suffix()
        ));
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join("config.yaml");
        let config = super::StoredAgentConfig {
            provider: "openai".to_string(),
            model: Some("gpt-test".to_string()),
            api_key: "secret-key".to_string(),
        };

        write_stored_config(&config_path, &config).unwrap();
        let credentials = root.join(".miao-agent/credentials.json");
        assert_eq!(
            fs::metadata(&credentials).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(fs::read_to_string(credentials)
            .unwrap()
            .contains("secret-key"));

        let runtime = root.join("runtime");
        fs::create_dir(&runtime).unwrap();
        write_runtime_files(&runtime, &config).unwrap();
        assert!(!runtime.join("config/auth.json").exists());
        assert!(!fs::read_to_string(runtime.join("config/settings.json"))
            .unwrap()
            .contains("secret-key"));

        let version = root.join(format!("v{}", super::PI_VERSION));
        fs::create_dir_all(version.join("theme")).unwrap();
        fs::File::create(version.join("pi"))
            .unwrap()
            .set_len(PI_ASSET.binary_size)
            .unwrap();
        assert!(!pi_is_installed_at(&root));

        fs::remove_dir_all(root).unwrap();
    }
}
