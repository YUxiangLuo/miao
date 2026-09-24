//! Personal AI gateway. Secrets live in a private, profile-owned file; the
//! ordinary panel never receives them. Kernel edits share config_update.
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    sync::{watch, Mutex, RwLock},
    task::JoinHandle,
};

use crate::{
    error::{AppError, AppResult},
    models::ai::*,
    state::AppState,
};

mod transport;
mod usage;
pub(crate) use transport::inference_router;

pub const INBOUND: &str = "miao-ai-in";
pub const OUTBOUND: &str = "miao-ai";

#[derive(Clone, Serialize, Deserialize)]
struct Provider {
    id: String,
    name: String,
    base_url: String,
    api_key: String,
    responses: bool,
    models: Vec<AiModel>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    revision: u64,
    enabled: bool,
    port: u16,
    proxy_port: u16,
    outbound: String,
    providers: Vec<Provider>,
    internal_key: String,
    access_hash: String,
}

impl Default for Stored {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: false,
            port: 6162,
            proxy_port: 6163,
            outbound: "proxy".into(),
            providers: Vec::new(),
            internal_key: String::new(),
            access_hash: String::new(),
        }
    }
}

impl Stored {
    fn view(&self) -> AiSettingsView {
        AiSettingsView {
            revision: self.revision,
            enabled: self.enabled,
            port: self.port,
            proxy_port: self.proxy_port,
            outbound: self.outbound.clone(),
            has_access_key: !self.access_hash.is_empty(),
            providers: self
                .providers
                .iter()
                .map(|p| AiProviderView {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    base_url: p.base_url.clone(),
                    has_key: !p.api_key.is_empty(),
                    responses: p.responses,
                    models: p.models.clone(),
                })
                .collect(),
        }
    }
}

struct Pool {
    generation: u64,
    outbound: String,
    client: reqwest::Client,
}

pub struct Gateway {
    stored: RwLock<Stored>,
    // Only configuration generation may see the staged kernel settings.
    pending: std::sync::RwLock<Option<Stored>>,
    admin_hash: RwLock<String>,
    pub error: RwLock<Option<String>>,
    listener: Mutex<Option<JoinHandle<()>>>,
    pool: Mutex<Option<Pool>>,
    cancel: watch::Sender<u64>,
    #[cfg(test)]
    tls_root: std::sync::Mutex<Option<reqwest::Certificate>>,
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            stored: RwLock::new(Stored::default()),
            pending: std::sync::RwLock::new(None),
            admin_hash: RwLock::new(String::new()),
            error: RwLock::new(None),
            listener: Mutex::new(None),
            pool: Mutex::new(None),
            cancel: watch::channel(0).0,
            #[cfg(test)]
            tls_root: std::sync::Mutex::new(None),
        }
    }
}

fn directory(state: &AppState) -> PathBuf {
    crate::paths::profile_state_dir(&state.config_path).join("ai")
}

fn token() -> AppResult<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| AppError::message("无法生成安全随机密钥"))?;
    Ok(format!("miao_{}", hex::encode(bytes)))
}

fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn matches_token(expected: &str, value: &str) -> bool {
    !expected.is_empty() && bool::from(expected.as_bytes().ct_eq(hash(value).as_bytes()))
}

// Protect the directory before any secret is written. Tempfiles start as 0600
// on Unix; on Windows they inherit this protected owner/system-only DACL.
fn private_directory(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
                    SE_FILE_OBJECT,
                },
                GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION,
                PROTECTED_DACL_SECURITY_INFORMATION,
            },
        };
        let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)\0"
            .encode_utf16()
            .collect();
        let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            let mut dacl = ptr::null_mut();
            let mut present = 0;
            let mut defaulted = 0;
            let result =
                if GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
                    == 0
                {
                    Err(std::io::Error::last_os_error())
                } else {
                    let code = SetNamedSecurityInfoW(
                        name.as_ptr(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        dacl,
                        ptr::null(),
                    );
                    if code == 0 {
                        Ok(())
                    } else {
                        Err(std::io::Error::from_raw_os_error(code as i32))
                    }
                };
            LocalFree(descriptor);
            result?;
        }
    }
    Ok(())
}

async fn write_secret(path: PathBuf, bytes: Vec<u8>) -> AppResult<()> {
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;
        let parent = path.parent().unwrap();
        private_directory(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(&path).map_err(|e| e.error)?;
        // Atomic replacement is the commit point. Never report a pre-commit
        // failure after it; an unavailable directory sync is diagnostic only.
        #[cfg(unix)]
        if let Err(err) = std::fs::File::open(parent).and_then(|dir| dir.sync_all()) {
            tracing::warn!(%err, "AI settings directory sync failed after commit");
        }
        Ok(())
    })
    .await
    .map_err(|_| AppError::message("AI 配置写入任务失败"))?
    .map_err(|_| AppError::message("无法安全保存 AI 配置，请检查目录权限和磁盘空间"))
}

pub async fn initialize(state: &Arc<AppState>) -> AppResult<()> {
    let dir = directory(state);
    private_directory(&dir)?;
    let admin_path = dir.join("admin-token.txt");
    let admin = match tokio::fs::read_to_string(&admin_path).await {
        Ok(value) if value.trim().len() >= 32 => value.trim().to_owned(),
        Ok(_) => return Err(AppError::message("AI 管理令牌文件无效")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let value = token()?;
            write_secret(admin_path, value.as_bytes().to_vec()).await?;
            value
        }
        Err(_) => return Err(AppError::message("无法读取 AI 管理令牌")),
    };
    *state.ai.admin_hash.write().await = hash(&admin);
    let stored = match tokio::fs::read(dir.join("settings.json")).await {
        Ok(bytes) => serde_json::from_slice::<Stored>(&bytes)
            .map_err(|_| AppError::message("AI 配置文件无效，未启动网关"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Stored {
            internal_key: token()?,
            ..Stored::default()
        },
        Err(_) => return Err(AppError::message("无法读取 AI 配置文件")),
    };
    validate_stored(&stored)?;
    let enabled = stored.enabled;
    let port = stored.port;
    *state.ai.stored.write().await = stored;
    if enabled {
        let listener = bind(port).await?;
        start_listener(state, listener).await;
    }
    Ok(())
}

async fn bind(port: u16) -> AppResult<TcpListener> {
    TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| AppError::message("AI API 端口无法绑定，请更换端口或停止占用它的程序"))
}

async fn start_listener(state: &Arc<AppState>, listener: TcpListener) {
    let mut task = state.ai.listener.lock().await;
    if let Some(old) = task.take() {
        old.abort();
        let _ = old.await;
    }
    let app = inference_router(state.clone());
    *task = Some(tokio::spawn(async move {
        if axum::serve(listener, app).await.is_err() {
            tracing::warn!("AI API listener stopped");
        }
    }));
}

pub async fn shutdown(state: &AppState) {
    state.ai.cancel.send_modify(|generation| *generation += 1);
    if let Some(task) = state.ai.listener.lock().await.take() {
        task.abort();
        let _ = task.await;
    }
    *state.ai.pool.lock().await = None;
}

pub async fn authorize(state: &AppState, value: &str) -> bool {
    matches_token(state.ai.admin_hash.read().await.as_str(), value)
}

pub async fn status(state: &AppState) -> AiStatus {
    let _guard = state.config_update.lock().await;
    let stored = state.ai.stored.read().await.clone();
    let mut error = state.ai.error.read().await.clone();
    let listening = state
        .ai
        .listener
        .lock()
        .await
        .as_ref()
        .is_some_and(|task| !task.is_finished());
    if stored.enabled && error.is_none() {
        if !listening {
            error = Some("AI API 监听未启动，请重新保存配置".into());
        } else if let Err(err) = transport::prepare_client(state, &stored).await {
            error = Some(err.to_string());
        }
    }
    AiStatus {
        enabled: stored.enabled,
        port: stored.port,
        outbound: stored.outbound.clone(),
        ready: stored.enabled && listening && error.is_none(),
        error,
        admin_token_path: directory(state)
            .join("admin-token.txt")
            .display()
            .to_string(),
    }
}

pub async fn settings(state: &AppState) -> AiSettingsView {
    state.ai.stored.read().await.view()
}

fn validate_stored(stored: &Stored) -> AppResult<()> {
    let bad = |message: &str| Err(AppError::message(message));
    if stored.port < 1024 || stored.proxy_port < 1024 || stored.port == stored.proxy_port {
        return bad("API 与内部代理端口必须是不同的 1024–65535 端口");
    }
    if stored.internal_key.len() < 32 {
        return bad("内部代理密钥无效");
    }
    if stored.outbound.is_empty() || [OUTBOUND, INBOUND].contains(&stored.outbound.as_str()) {
        return bad("AI 出口无效");
    }
    if stored.providers.len() > 32 {
        return bad("最多配置 32 个供应商");
    }
    let mut ids = HashSet::new();
    let mut models = HashSet::new();
    for provider in &stored.providers {
        if provider.id.is_empty()
            || provider.id.len() > 64
            || !provider
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            || !ids.insert(&provider.id)
        {
            return bad("供应商 ID 必须唯一，且仅含字母、数字、连字符或下划线");
        }
        if provider.name.trim().is_empty() || provider.name.len() > 128 {
            return bad("供应商名称无效");
        }
        let url = url::Url::parse(&provider.base_url)
            .map_err(|_| AppError::message("供应商 Base URL 无效"))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return bad("Base URL 必须为 HTTPS，不能包含凭据、查询参数或片段");
        }
        if provider.api_key.is_empty()
            || provider.api_key.len() > 8192
            || reqwest::header::HeaderValue::from_str(&format!("Bearer {}", provider.api_key))
                .is_err()
        {
            return bad("供应商 API Key 无效");
        }
        if provider.models.is_empty() || provider.models.len() > 256 {
            return bad("每个供应商需要 1–256 个模型");
        }
        for model in &provider.models {
            if model.id.trim().is_empty()
                || model.upstream.trim().is_empty()
                || model.id.len() > 256
                || model.upstream.len() > 512
                || !models.insert(&model.id)
            {
                return bad("公开模型 ID 必须唯一，且模型名称不能为空");
            }
        }
    }
    if stored.enabled && (stored.providers.is_empty() || stored.access_hash.is_empty()) {
        return bad("启用前请配置供应商、模型并生成客户端密钥");
    }
    Ok(())
}

pub async fn rotate_access_key(state: &Arc<AppState>) -> AppResult<AiAccessKey> {
    let _lock = state.config_update.lock().await;
    if state.lifecycle.is_shutting_down() {
        return Err(AppError::message("服务正在关闭"));
    }
    let mut next = state.ai.stored.read().await.clone();
    if next.internal_key.is_empty() {
        return Err(AppError::message("AI 网关未初始化"));
    }
    let key = token()?;
    next.access_hash = hash(&key);
    next.revision += 1;
    write_secret(
        directory(state).join("settings.json"),
        serde_json::to_vec(&next)?,
    )
    .await?;
    let revision = next.revision;
    *state.ai.stored.write().await = next;
    Ok(AiAccessKey { key, revision })
}

pub async fn save(state: &Arc<AppState>, input: AiSettingsInput) -> AppResult<AiSettingsView> {
    let edit = super::config::ConfigEdit::begin(state).await;
    if state.lifecycle.is_shutting_down()
        || state
            .initializing
            .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Err(AppError::message("服务正在初始化或关闭"));
    }
    let old = state.ai.stored.read().await.clone();
    if input.revision != old.revision {
        return Err(AppError::message("AI 配置已变化，请重新加载后保存"));
    }
    let providers = input
        .providers
        .into_iter()
        .map(|p| {
            let api_key = p
                .api_key
                .or_else(|| {
                    old.providers
                        .iter()
                        .find(|old| old.id == p.id)
                        .map(|old| old.api_key.clone())
                })
                .unwrap_or_default();
            Provider {
                id: p.id,
                name: p.name.trim().into(),
                base_url: p.base_url.trim().trim_end_matches('/').into(),
                api_key,
                responses: p.responses,
                models: p.models,
            }
        })
        .collect();
    let next = Stored {
        revision: old.revision + 1,
        enabled: input.enabled,
        port: input.port,
        proxy_port: input.proxy_port,
        outbound: input.outbound,
        providers,
        ..old.clone()
    };
    validate_stored(&next)?;
    let panel_port = state
        .config
        .read()
        .await
        .port
        .unwrap_or(crate::models::DEFAULT_PORT);
    let clash_port = super::singbox::CLASH_API_HOST
        .parse::<std::net::SocketAddr>()
        .unwrap()
        .port();
    if [next.port, next.proxy_port]
        .iter()
        .any(|port| *port == panel_port || *port == clash_port)
    {
        return Err(AppError::message("AI 端口不能占用面板或 Clash API 端口"));
    }
    let targets = super::config::known_rule_targets(edit.original(), state).await;
    let known = |target: &str| {
        target == "proxy" || target == "direct" || targets.iter().any(|name| name == target)
    };
    if next.enabled && !known(&next.outbound) {
        return Err(AppError::message("所选 AI 出口不存在"));
    }
    if next.enabled && edit.original().nodes.is_empty() && edit.original().subs.is_empty() {
        return Err(AppError::message(
            "请先配置代理节点或订阅，AI 网关需要代理内核",
        ));
    }
    let listener_missing = state
        .ai
        .listener
        .lock()
        .await
        .as_ref()
        .is_none_or(|task| task.is_finished());
    let listener = if next.enabled && (!old.enabled || old.port != next.port || listener_missing) {
        Some(bind(next.port).await?)
    } else {
        None
    };
    let route_blocked = tokio::fs::read(&state.runtime_paths.active_config)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|config| {
            config["route"]["rules"].as_array().is_some_and(|rules| {
                rules
                    .iter()
                    .any(|rule| rule["inbound"] == json!([INBOUND]) && rule["action"] == "reject")
            })
        });
    let kernel_changed = old.enabled != next.enabled
        || state.ai.error.read().await.is_some()
        || (next.enabled && (old.proxy_port != next.proxy_port || route_blocked));
    // Never leave staged settings visible if this future is cancelled.
    struct ClearPending<'a>(&'a std::sync::RwLock<Option<Stored>>);
    impl Drop for ClearPending<'_> {
        fn drop(&mut self) {
            *self.0.write().unwrap() = None;
        }
    }
    let _pending = ClearPending(&state.ai.pending);
    if kernel_changed {
        *state.ai.pending.write().unwrap() = Some(next.clone());
        if let Err(err) = edit.apply().await {
            *state.ai.pending.write().unwrap() = None;
            *state.ai.error.write().await =
                Some("AI 内部代理配置应用失败，请重新保存配置恢复".into());
            return Err(AppError::context("AI 内部代理配置应用失败", err));
        }
    }
    if let Err(err) = write_secret(
        directory(state).join("settings.json"),
        serde_json::to_vec(&next)?,
    )
    .await
    {
        *state.ai.pending.write().unwrap() = None;
        if kernel_changed {
            if let Err(rollback) = edit.apply().await {
                *state.ai.error.write().await =
                    Some("AI 配置保存及回滚失败，请重新保存配置后再使用".into());
                return Err(AppError::message(format!("{err}; 回滚失败: {rollback}")));
            }
        }
        return Err(err);
    }
    *state.ai.pending.write().unwrap() = None;
    *state.ai.stored.write().await = next.clone();
    *state.ai.pool.lock().await = None;
    *state.ai.error.write().await = None;
    if !next.enabled || state.lifecycle.is_shutting_down() {
        shutdown(state).await;
    } else if let Some(listener) = listener {
        start_listener(state, listener).await;
    }
    state
        .data_revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Ok(next.view())
}

impl Gateway {
    pub async fn reserved_tags(&self) -> Vec<String> {
        let settings = self.kernel_settings().await;
        vec![INBOUND.into(), OUTBOUND.into(), settings.outbound]
    }

    async fn kernel_settings(&self) -> Stored {
        if let Some(staged) = self.pending.read().unwrap().as_ref() {
            return staged.clone();
        }
        self.stored.read().await.clone()
    }

    pub async fn inject(&self, config: &mut Value) -> AppResult<()> {
        let settings = self.kernel_settings().await;
        if !settings.enabled {
            return Ok(());
        }
        let outbounds = config["outbounds"]
            .as_array_mut()
            .ok_or_else(|| AppError::message("缺少代理出口配置"))?;
        let mut targets: Vec<String> = outbounds
            .iter()
            .filter_map(|v| v["tag"].as_str().map(str::to_owned))
            .collect();
        if targets.iter().any(|tag| tag == OUTBOUND || tag == INBOUND) {
            return Err(AppError::message("节点名称与 AI 网关保留名称冲突"));
        }
        targets.sort();
        targets.dedup();
        let available = targets.contains(&settings.outbound);
        let default = if available {
            &settings.outbound
        } else {
            "proxy"
        };
        outbounds.push(json!({"type":"selector", "tag":OUTBOUND, "outbounds":targets, "default":default, "interrupt_exist_connections":false}));
        config["inbounds"].as_array_mut().unwrap().push(json!({"type":"http", "tag":INBOUND, "listen":"127.0.0.1", "listen_port":settings.proxy_port,
            "users":[{"username":"miao", "password":settings.internal_key}]}));
        let rule = if available {
            json!({"inbound":[INBOUND],"action":"route","outbound":OUTBOUND})
        } else {
            json!({"inbound":[INBOUND],"action":"reject"})
        };
        config["route"]["rules"]
            .as_array_mut()
            .unwrap()
            .insert(0, rule);
        Ok(())
    }

    // Old caches must not enable a removed gateway or restore old credentials,
    // ports or routing. This check also covers pre-gateway legacy caches.
    pub async fn cache_matches(&self, bytes: &[u8]) -> bool {
        let Ok(mut config) = serde_json::from_slice::<Value>(bytes) else {
            return false;
        };
        if !self.kernel_settings().await.enabled {
            return !config["inbounds"]
                .as_array()
                .is_some_and(|values| values.iter().any(|v| v["tag"] == INBOUND))
                && !config["outbounds"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|v| v["tag"] == OUTBOUND));
        }
        let original = config.clone();
        if let Some(values) = config["inbounds"].as_array_mut() {
            values.retain(|v| v["tag"] != INBOUND);
        }
        if let Some(values) = config["outbounds"].as_array_mut() {
            values.retain(|v| v["tag"] != OUTBOUND);
        }
        if let Some(values) = config["route"]["rules"].as_array_mut() {
            values.retain(|v| v["inbound"] != json!([INBOUND]));
        }
        if self.kernel_settings().await.enabled
            && (config["inbounds"].as_array().is_none()
                || config["route"]["rules"].as_array().is_none())
        {
            return false;
        }
        self.inject(&mut config).await.is_ok() && config == original
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "linux"))]
mod e2e;
#[cfg(all(test, target_os = "linux"))]
#[path = "../../tests/ai_transport.rs"]
mod fixture;
