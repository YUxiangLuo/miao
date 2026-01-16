use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::{delete, get, post},
    Router,
};
use lazy_static::lazy_static;
use nix::sys::signal::{kill, Signal};
use nix::unistd::{Pid, Uid};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

// Version embedded at compile time
const VERSION: &str = env!("CARGO_PKG_VERSION");

// Embed sing-box binary based on target architecture
#[cfg(target_arch = "x86_64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../embedded/sing-box-amd64");

#[cfg(target_arch = "aarch64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../embedded/sing-box-arm64");

const IP_RULE_BINARY: &[u8] = include_bytes!("../embedded/geoip-cn.srs");
const SITE_RULE_BINARY: &[u8] = include_bytes!("../embedded/geosite-geolocation-cn.srs");

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(default)]
    subs: Vec<String>,
    #[serde(default)]
    nodes: Vec<String>,
}

const DEFAULT_PORT: u16 = 6161;

struct AppState {
    config: Mutex<Config>,
}

#[derive(Serialize, Deserialize)]
struct Hysteria2 {
    #[serde(rename = "type")]
    outbound_type: String,
    tag: String,
    server: String,
    server_port: u16,
    password: String,
    up_mbps: u32,
    down_mbps: u32,
    tls: Tls,
}

#[derive(Serialize, Deserialize)]
struct Tls {
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_name: Option<String>,
    insecure: bool,
}

#[derive(Serialize, Deserialize)]
struct AnyTls {
    #[serde(rename = "type")]
    outbound_type: String,
    tag: String,
    server: String,
    server_port: u16,
    password: String,
    tls: Tls,
}

#[derive(Serialize, Deserialize)]
struct Shadowsocks {
    #[serde(rename = "type")]
    outbound_type: String,
    tag: String,
    server: String,
    server_port: u16,
    method: String,
    password: String,
}

// ============================================================================
// API Response Types
// ============================================================================

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    fn success(message: impl Into<String>, data: T) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
        }
    }

    fn success_no_data(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Serialize)]
struct StatusData {
    running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uptime_secs: Option<u64>,
}

#[derive(Serialize, Clone)]
struct ConnectivityResult {
    name: String,
    url: String,
    latency_ms: Option<u64>,
    success: bool,
}

// Request types for subscription and node management
#[derive(Deserialize)]
struct SubRequest {
    url: String,
}

#[derive(Deserialize)]
struct NodeRequest {
    node_type: Option<String>,
    tag: String,
    server: String,
    server_port: u16,
    password: String,
    #[serde(default)]
    sni: Option<String>,
    #[serde(default)]
    cipher: Option<String>,
}

#[derive(Deserialize)]
struct DeleteNodeRequest {
    tag: String,
}

#[derive(Serialize)]
struct NodeInfo {
    tag: String,
    server: String,
    server_port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    sni: Option<String>,
}

// ============================================================================
// Global State
// ============================================================================

struct SingBoxProcess {
    child: tokio::process::Child,
    started_at: Instant,
}

#[derive(Clone, Serialize)]
struct SubStatus {
    url: String,
    success: bool,
    node_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

lazy_static! {
    static ref SING_PROCESS: Mutex<Option<SingBoxProcess>> = Mutex::new(None);
    static ref SUB_STATUS: Mutex<HashMap<String, SubStatus>> = Mutex::new(HashMap::new());
}

// ============================================================================
// API Handlers
// ============================================================================

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../public/index.html"))
}

async fn serve_favicon() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "image/svg+xml")], include_str!("../public/icon.svg"))
}

async fn serve_vue() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "application/javascript")], include_str!("../embedded/assets/vue.js"))
}

async fn serve_bootstrap_css() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "text/css")], include_str!("../embedded/assets/bootstrap-icons.css"))
}

async fn serve_font_woff2() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static [u8]) {
    ([(axum::http::header::CONTENT_TYPE, "font/woff2")], include_bytes!("../embedded/assets/fonts/bootstrap-icons.woff2"))
}

async fn serve_font_woff() -> ([(axum::http::header::HeaderName, &'static str); 1], &'static [u8]) {
    ([(axum::http::header::CONTENT_TYPE, "font/woff")], include_bytes!("../embedded/assets/fonts/bootstrap-icons.woff"))
}

/// GET /api/status - Get sing-box running status
async fn get_status() -> Json<ApiResponse<StatusData>> {
    let mut lock = SING_PROCESS.lock().await;

    let (running, pid, uptime_secs) = if let Some(ref mut proc) = *lock {
        match proc.child.try_wait() {
            Ok(Some(_)) => {
                *lock = None;
                (false, None, None)
            }
            Ok(None) => {
                let uptime = proc.started_at.elapsed().as_secs();
                (true, proc.child.id(), Some(uptime))
            }
            Err(_) => (false, None, None),
        }
    } else {
        (false, None, None)
    };

    Json(ApiResponse::success(
        if running { "running" } else { "stopped" },
        StatusData {
            running,
            pid,
            uptime_secs,
        },
    ))
}

/// POST /api/service/start - Start sing-box
async fn start_service(
    State(_): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let mut lock = SING_PROCESS.lock().await;

    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait().ok().flatten().is_none() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("sing-box is already running")),
            ));
        }
    }

    drop(lock);

    match start_sing_internal().await {
        Ok(_) => {
            // Try to restore last selected proxy (non-blocking)
            tokio::spawn(async {
                restore_last_proxy().await;
            });
            Ok(Json(ApiResponse::success_no_data("sing-box started successfully")))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(format!("Failed to start: {}", e))),
        )),
    }
}

/// POST /api/service/stop - Stop sing-box
async fn stop_service() -> Json<ApiResponse<()>> {
    stop_sing_internal().await;
    Json(ApiResponse::success_no_data("sing-box stopped"))
}

/// POST /api/connectivity - Test connectivity to a single site
#[derive(Deserialize)]
struct ConnectivityRequest {
    url: String,
}

async fn test_connectivity(
    Json(req): Json<ConnectivityRequest>,
) -> Json<ApiResponse<ConnectivityResult>> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return Json(ApiResponse::error(format!("Failed to create client: {}", e)));
        }
    };

    let start = Instant::now();
    let result = match client.head(&req.url).send().await {
        Ok(_) => ConnectivityResult {
            name: String::new(),
            url: req.url,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            success: true,
        },
        Err(_) => ConnectivityResult {
            name: String::new(),
            url: req.url,
            latency_ms: None,
            success: false,
        },
    };

    Json(ApiResponse::success("Test completed", result))
}

// ============================================================================
// Version and Upgrade APIs
// ============================================================================

#[derive(Serialize)]
struct VersionInfo {
    current: String,
    latest: Option<String>,
    has_update: bool,
    download_url: Option<String>,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Parse version string like "v0.6.10" into comparable tuple
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

/// Compare two version strings, returns true if `latest` is newer than `current`
fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// GET /api/version - Get current version and check for updates
async fn get_version() -> Json<ApiResponse<VersionInfo>> {
    let current = format!("v{}", VERSION);

    // Try to fetch latest version from GitHub
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => {
            return Json(ApiResponse::success("Version info", VersionInfo {
                current,
                latest: None,
                has_update: false,
                download_url: None,
            }));
        }
    };

    let resp = client
        .get("https://api.github.com/repos/YUxiangLuo/miao/releases/latest")
        .header("User-Agent", "miao")
        .send()
        .await;

    match resp {
        Ok(r) => {
            if let Ok(release) = r.json::<GitHubRelease>().await {
                let latest = release.tag_name.clone();
                let has_update = is_newer_version(&current, &latest);

                // Find download URL for current architecture
                let asset_name = if cfg!(target_arch = "x86_64") {
                    "miao-rust-linux-amd64"
                } else if cfg!(target_arch = "aarch64") {
                    "miao-rust-linux-arm64"
                } else {
                    ""
                };

                let download_url = release.assets.iter()
                    .find(|a| a.name == asset_name)
                    .map(|a| a.browser_download_url.clone());

                Json(ApiResponse::success("Version info", VersionInfo {
                    current,
                    latest: Some(latest),
                    has_update,
                    download_url,
                }))
            } else {
                Json(ApiResponse::success("Version info", VersionInfo {
                    current,
                    latest: None,
                    has_update: false,
                    download_url: None,
                }))
            }
        }
        Err(_) => {
            Json(ApiResponse::success("Version info", VersionInfo {
                current,
                latest: None,
                has_update: false,
                download_url: None,
            }))
        }
    }
}

/// POST /api/upgrade - Download and apply upgrade
async fn upgrade() -> Json<ApiResponse<String>> {
    // 1. Fetch latest release info
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build() {
        Ok(c) => c,
        Err(e) => return Json(ApiResponse::error(format!("Failed to create HTTP client: {}", e))),
    };

    let release: GitHubRelease = match client
        .get("https://api.github.com/repos/YUxiangLuo/miao/releases/latest")
        .header("User-Agent", "miao")
        .send()
        .await {
        Ok(r) => match r.json().await {
            Ok(rel) => rel,
            Err(e) => return Json(ApiResponse::error(format!("Failed to parse release info: {}", e))),
        },
        Err(e) => return Json(ApiResponse::error(format!("Failed to fetch release info: {}", e))),
    };

    let current = format!("v{}", VERSION);
    if !is_newer_version(&current, &release.tag_name) {
        return Json(ApiResponse::success_no_data("Already up to date"));
    }

    // 2. Find download URL for current architecture
    let asset_name = if cfg!(target_arch = "x86_64") {
        "miao-rust-linux-amd64"
    } else if cfg!(target_arch = "aarch64") {
        "miao-rust-linux-arm64"
    } else {
        return Json(ApiResponse::error("Unsupported architecture"));
    };

    let download_url = match release.assets.iter().find(|a| a.name == asset_name) {
        Some(a) => a.browser_download_url.clone(),
        None => return Json(ApiResponse::error("No binary found for current architecture")),
    };

    // 3. Download new binary to temp location
    println!("Downloading update from: {}", download_url);
    let binary_data = match client.get(&download_url).send().await {
        Ok(r) => match r.bytes().await {
            Ok(b) => b,
            Err(e) => return Json(ApiResponse::error(format!("Failed to download binary: {}", e))),
        },
        Err(e) => return Json(ApiResponse::error(format!("Failed to download: {}", e))),
    };

    let temp_path = "/tmp/miao-new";
    if let Err(e) = fs::write(temp_path, &binary_data) {
        return Json(ApiResponse::error(format!("Failed to write temp file: {}", e)));
    }

    // 4. Make it executable
    if let Err(e) = fs::set_permissions(temp_path, fs::Permissions::from_mode(0o755)) {
        return Json(ApiResponse::error(format!("Failed to set permissions: {}", e)));
    }

    // 5. Verify the new binary can run
    let verify = tokio::process::Command::new(temp_path)
        .arg("--help")
        .output()
        .await;

    if verify.is_err() {
        let _ = fs::remove_file(temp_path);
        return Json(ApiResponse::error("New binary verification failed"));
    }

    // 6. Get current executable path
    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return Json(ApiResponse::error(format!("Failed to get current exe path: {}", e))),
    };

    // 7. Stop sing-box before replacing and wait for it to exit
    println!("Stopping sing-box before upgrade...");
    stop_sing_internal().await;

    // 8. Backup current binary (must succeed)
    let backup_path = format!("{}.bak", current_exe.display());
    if let Err(e) = fs::copy(&current_exe, &backup_path) {
        return Json(ApiResponse::error(format!("Failed to backup current binary: {}", e)));
    }

    // 9. Replace binary: delete first then copy (Linux allows deleting running executables)
    if let Err(e) = fs::remove_file(&current_exe) {
        return Json(ApiResponse::error(format!("Failed to remove old binary: {}", e)));
    }
    if let Err(e) = fs::copy(temp_path, &current_exe) {
        // Try to restore from backup
        let _ = fs::copy(&backup_path, &current_exe);
        return Json(ApiResponse::error(format!("Failed to copy new binary: {}", e)));
    }
    // Set executable permission
    if let Err(e) = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755)) {
        // Try to restore from backup
        let _ = fs::remove_file(&current_exe);
        let _ = fs::copy(&backup_path, &current_exe);
        return Json(ApiResponse::error(format!("Failed to set permissions: {}", e)));
    }
    let _ = fs::remove_file(temp_path);

    println!("Upgrade successful! Restarting...");

    // 10. Exec to restart with new binary
    let new_version = release.tag_name.clone();
    let sing_box_home = get_sing_box_home();
    tokio::spawn(async move {
        sleep(Duration::from_millis(500)).await;

        // Clean up embedded files so new version will re-extract them
        // Keep .last_proxy to preserve user's last selected node
        let files_to_remove = ["sing-box", "chinaip.srs", "chinasite.srs"];
        for file in &files_to_remove {
            let path = sing_box_home.join(file);
            if path.exists() {
                println!("Removing old file: {:?}", path);
                let _ = fs::remove_file(&path);
            }
        }

        use std::os::unix::process::CommandExt;
        let args: Vec<String> = std::env::args().collect();
        let err = std::process::Command::new(&current_exe)
            .args(&args[1..])
            .exec();

        // exec() only returns if there's an error, try to restore from backup
        eprintln!("Failed to exec new binary: {}", err);
        eprintln!("Attempting to restore from backup...");

        if fs::remove_file(&current_exe).is_ok() {
            if fs::copy(&backup_path, &current_exe).is_ok() {
                let _ = fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755));
                eprintln!("Restored from backup, restarting with old version...");
                let _ = std::process::Command::new(&current_exe)
                    .args(&args[1..])
                    .exec();
            }
        }
        eprintln!("Failed to restore from backup, manual intervention required");
        std::process::exit(1);
    });

    Json(ApiResponse::success("Upgrade complete, restarting...", new_version))
}

// ============================================================================
// Subscription Management APIs
// ============================================================================

/// GET /api/subs - Get all subscription URLs with status
async fn get_subs(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<SubStatus>>> {
    let config = state.config.lock().await;
    let status_map = SUB_STATUS.lock().await;

    let subs_with_status: Vec<SubStatus> = config
        .subs
        .iter()
        .map(|url| {
            status_map.get(url).cloned().unwrap_or(SubStatus {
                url: url.clone(),
                success: true, // Not yet fetched, assume pending
                node_count: 0,
                error: None,
            })
        })
        .collect();

    Json(ApiResponse::success("Subscriptions loaded", subs_with_status))
}

/// POST /api/subs - Add a subscription URL
async fn add_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let config_clone;
    {
        let mut config = state.config.lock().await;

        if config.subs.contains(&req.url) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::error("Subscription already exists")),
            ));
        }

        config.subs.push(req.url);

        if let Err(e) = save_config(&config).await {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!("Failed to save config: {}", e))),
            ));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone).await {
        Ok(_) => Ok(Json(ApiResponse::success_no_data("Subscription added and sing-box restarted"))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error(e)))),
    }
}

/// DELETE /api/subs - Delete a subscription URL
async fn delete_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let config_clone;
    {
        let mut config = state.config.lock().await;

        let original_len = config.subs.len();
        config.subs.retain(|s| s != &req.url);

        if config.subs.len() == original_len {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Subscription not found")),
            ));
        }

        if let Err(e) = save_config(&config).await {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!("Failed to save config: {}", e))),
            ));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone).await {
        Ok(_) => Ok(Json(ApiResponse::success_no_data("Subscription deleted and sing-box restarted"))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error(e)))),
    }
}

/// POST /api/subs/refresh - Refresh subscriptions and restart
async fn refresh_subs(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let config = state.config.lock().await;
    let config_clone = config.clone();
    drop(config);

    match regenerate_and_restart(&config_clone).await {
        Ok(_) => Ok(Json(ApiResponse::success_no_data("Subscriptions refreshed and sing-box restarted"))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(e)),
        )),
    }
}

// ============================================================================
// Node Management APIs
// ============================================================================

/// GET /api/nodes - Get all manual nodes
async fn get_nodes(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<NodeInfo>>> {
    let config = state.config.lock().await;

    let nodes: Vec<NodeInfo> = config
        .nodes
        .iter()
        .filter_map(|s| {
            serde_json::from_str::<serde_json::Value>(s).ok().map(|v| NodeInfo {
                tag: v.get("tag").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                server: v.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                server_port: v.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0) as u16,
                sni: v
                    .get("tls")
                    .and_then(|t| t.get("server_name"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect();

    Json(ApiResponse::success("Nodes loaded", nodes))
}

/// POST /api/nodes - Add a Hysteria2 node
async fn add_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NodeRequest>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let config_clone;
    {
        let mut config = state.config.lock().await;

        // Check if tag already exists
        for node_str in &config.nodes {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(node_str) {
                if v.get("tag").and_then(|t| t.as_str()) == Some(&req.tag) {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ApiResponse::error("Node with this tag already exists")),
                    ));
                }
            }
        }

        // Build node based on type
        let node_type = req.node_type.as_deref().unwrap_or("hysteria2");
        let node_json = match node_type {
            "anytls" => {
                let node = AnyTls {
                    outbound_type: "anytls".to_string(),
                    tag: req.tag,
                    server: req.server,
                    server_port: req.server_port,
                    password: req.password,
                    tls: Tls {
                        enabled: true,
                        server_name: req.sni,
                        insecure: true,
                    },
                };
                serde_json::to_string(&node)
            }
            "ss" => {
                let node = Shadowsocks {
                    outbound_type: "shadowsocks".to_string(),
                    tag: req.tag,
                    server: req.server,
                    server_port: req.server_port,
                    method: req.cipher.unwrap_or_else(|| "2022-blake3-aes-128-gcm".to_string()),
                    password: req.password,
                };
                serde_json::to_string(&node)
            }
            _ => {
                // Default to Hysteria2
                let node = Hysteria2 {
                    outbound_type: "hysteria2".to_string(),
                    tag: req.tag,
                    server: req.server,
                    server_port: req.server_port,
                    password: req.password,
                    up_mbps: 40,
                    down_mbps: 350,
                    tls: Tls {
                        enabled: true,
                        server_name: req.sni,
                        insecure: true,
                    },
                };
                serde_json::to_string(&node)
            }
        }.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!("Failed to serialize node: {}", e))),
            )
        })?;

        config.nodes.push(node_json);

        if let Err(e) = save_config(&config).await {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!("Failed to save config: {}", e))),
            ));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone).await {
        Ok(_) => Ok(Json(ApiResponse::success_no_data("Node added and sing-box restarted"))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error(e)))),
    }
}

/// DELETE /api/nodes - Delete a node by tag
async fn delete_node(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteNodeRequest>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let config_clone;
    {
        let mut config = state.config.lock().await;

        let original_len = config.nodes.len();
        config.nodes.retain(|node_str| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(node_str) {
                v.get("tag").and_then(|t| t.as_str()) != Some(&req.tag)
            } else {
                true
            }
        });

        if config.nodes.len() == original_len {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ApiResponse::error("Node not found")),
            ));
        }

        if let Err(e) = save_config(&config).await {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::error(format!("Failed to save config: {}", e))),
            ));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone).await {
        Ok(_) => Ok(Json(ApiResponse::success_no_data("Node deleted and sing-box restarted"))),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse::error(e)))),
    }
}

// ============================================================================
// Last Proxy Management (for auto-restore after restart)
// ============================================================================

#[derive(Serialize, Deserialize, Clone)]
struct LastProxy {
    group: String,
    name: String,
}

fn get_last_proxy_path() -> PathBuf {
    get_sing_box_home().join(".last_proxy")
}

async fn save_last_proxy(proxy: &LastProxy) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::to_string(proxy)?;
    tokio::fs::write(get_last_proxy_path(), json).await?;
    Ok(())
}

async fn load_last_proxy() -> Option<LastProxy> {
    let path = get_last_proxy_path();
    if let Ok(content) = tokio::fs::read_to_string(&path).await {
        serde_json::from_str(&content).ok()
    } else {
        None
    }
}

/// POST /api/last-proxy - Save last selected proxy
async fn set_last_proxy(
    Json(req): Json<LastProxy>,
) -> Json<ApiResponse<()>> {
    match save_last_proxy(&req).await {
        Ok(_) => Json(ApiResponse::success_no_data("Last proxy saved")),
        Err(e) => Json(ApiResponse::error(format!("Failed to save: {}", e))),
    }
}

/// Try to restore last selected proxy via Clash API
async fn restore_last_proxy() {
    let proxy = match load_last_proxy().await {
        Some(p) => p,
        None => return,
    };

    println!("Attempting to restore last proxy: {} -> {}", proxy.group, proxy.name);

    // Wait a bit for Clash API to be ready
    sleep(Duration::from_secs(1)).await;

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };

    // First check if the node exists in the group
    let check_url = format!("http://127.0.0.1:6262/proxies/{}", urlencoding::encode(&proxy.group));
    let group_info = match client.get(&check_url).send().await {
        Ok(res) => match res.json::<serde_json::Value>().await {
            Ok(v) => v,
            Err(_) => return,
        },
        Err(_) => return,
    };

    // Check if the node name exists in the group's "all" array
    let all_nodes = group_info.get("all").and_then(|v| v.as_array());
    if let Some(nodes) = all_nodes {
        let node_exists = nodes.iter().any(|n| n.as_str() == Some(&proxy.name));
        if !node_exists {
            println!("Last proxy '{}' not found in current node list, skipping restore", proxy.name);
            return;
        }
    } else {
        return;
    }

    // Restore the proxy selection
    let url = format!("http://127.0.0.1:6262/proxies/{}", urlencoding::encode(&proxy.group));
    match client
        .put(&url)
        .json(&serde_json::json!({ "name": proxy.name }))
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            println!("Successfully restored last proxy: {}", proxy.name);
        }
        Ok(res) => {
            println!("Failed to restore last proxy: status {}", res.status());
        }
        Err(e) => {
            println!("Failed to restore last proxy: {}", e);
        }
    }
}

// ============================================================================
// Internal Functions
// ============================================================================

/// Save config to config.yaml
async fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let yaml = serde_yaml::to_string(config)?;
    tokio::fs::write("config.yaml", yaml).await?;
    Ok(())
}

/// Regenerate sing-box config and restart the service
async fn regenerate_and_restart(config: &Config) -> Result<(), String> {
    // Regenerate config
    gen_config(config).await.map_err(|e| format!("Failed to regenerate config: {}", e))?;
    println!("Config regenerated successfully");

    // Stop and restart sing-box
    stop_sing_internal().await;
    sleep(Duration::from_millis(500)).await;

    start_sing_internal().await.map_err(|e| format!("Failed to restart sing-box: {}", e))?;
    println!("sing-box restarted successfully");

    // Try to restore last selected proxy (non-blocking)
    tokio::spawn(async {
        restore_last_proxy().await;
    });

    Ok(())
}

/// Extract embedded sing-box binary to /tmp (saves flash storage on OpenWrt)
/// Returns the path to sing-box home directory
fn extract_sing_box() -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let sing_box_home = PathBuf::from("/tmp/miao-sing-box");
    if !sing_box_home.exists() {
        fs::create_dir_all(&sing_box_home)?;
    }

    let sing_box_path = sing_box_home.join("sing-box");
    let ip_rule_path = sing_box_home.join("chinaip.srs");
    let site_rule_path = sing_box_home.join("chinasite.srs");

    if !sing_box_path.exists() {
        println!("Extracting embedded sing-box binary to {:?}", sing_box_path);
        fs::write(&sing_box_path, SING_BOX_BINARY)?;
        fs::set_permissions(&sing_box_path, fs::Permissions::from_mode(0o755))?;
        println!("sing-box binary extracted successfully");
    }

    // Always check and extract rule files separately
    if !ip_rule_path.exists() {
        println!("Extracting geoip rule file to {:?}", ip_rule_path);
        fs::write(&ip_rule_path, IP_RULE_BINARY)?;
    }
    if !site_rule_path.exists() {
        println!("Extracting geosite rule file to {:?}", site_rule_path);
        fs::write(&site_rule_path, SITE_RULE_BINARY)?;
    }

    let dashboard_dir = sing_box_home.join("dashboard");
    if !dashboard_dir.exists() {
        fs::create_dir_all(&dashboard_dir)?;
    }

    Ok(sing_box_home)
}

fn get_sing_box_home() -> PathBuf {
    PathBuf::from("/tmp/miao-sing-box")
}

async fn check_and_install_openwrt_dependencies(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !PathBuf::from("/etc/openwrt_release").exists() {
        return Ok(());
    }

    println!("OpenWrt system detected. Checking dependencies...");

    let output = tokio::process::Command::new("opkg")
        .arg("list-installed")
        .output()
        .await?;

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
        println!("Required dependencies (kmod-tun, kmod-nft-queue) are already installed.");
        return Ok(());
    }

    println!(
        "Missing dependencies: {:?}. Installing...",
        packages_to_install
    );

    println!("Running 'opkg update'...");
    let update_status = tokio::process::Command::new("opkg")
        .arg("update")
        .status()
        .await?;

    if !update_status.success() {
        eprintln!("'opkg update' finished with error, but proceeding with installation attempt...");
    }

    for pkg in packages_to_install {
        println!("Installing {}...", pkg);
        let install_status = tokio::process::Command::new("opkg")
            .arg("install")
            .arg(pkg)
            .status()
            .await?;

        if !install_status.success() {
            return Err(
                format!("Failed to install {}. Please install it manually.", pkg).into(),
            );
        }
    }

    println!("Dependencies installed successfully.");
    Ok(())
}

async fn start_sing_internal(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut lock = SING_PROCESS.lock().await;
    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait()?.is_none() {
            return Err("already running!".into());
        }
    }

    let sing_box_home = get_sing_box_home();
    let sing_box_path = sing_box_home.join("sing-box");
    let config_path = sing_box_home.join("config.json");

    println!("Starting sing-box from: {:?}", sing_box_path);
    println!("Using config: {:?}", config_path);

    let mut child = tokio::process::Command::new(&sing_box_path)
        .current_dir(&sing_box_home)
        .arg("run")
        .arg("-c")
        .arg(&config_path)
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;

    let pid = child.id();
    println!("sing-box process spawned with PID: {:?}", pid);

    // Wait a short moment to check if process exits immediately
    sleep(Duration::from_millis(500)).await;
    if let Some(exit_status) = child.try_wait()? {
        let code = exit_status.code().unwrap_or(-1);
        return Err(format!("sing-box exited immediately with code {}", code).into());
    }

    // Store the process first
    *lock = Some(SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    drop(lock); // Release lock before connectivity check

    // Wait for sing-box to fully initialize
    sleep(Duration::from_secs(5)).await;

    // Connectivity check (3 attempts)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    
    let mut connectivity_ok = false;
    for attempt in 1..=3 {
        println!("Connectivity check attempt {}/3...", attempt);
        match client.get("http://connectivitycheck.gstatic.com/generate_204").send().await {
            Ok(res) if res.status().as_u16() == 204 => {
                println!("Connectivity check passed!");
                connectivity_ok = true;
                break;
            }
            Ok(res) => {
                println!("Connectivity check failed: status {}", res.status());
            }
            Err(e) => {
                println!("Connectivity check failed: {}", e);
            }
        }
        if attempt < 3 {
            sleep(Duration::from_secs(2)).await;
        }
    }

    if !connectivity_ok {
        // Stop sing-box gracefully and clean up
        println!("Connectivity check failed, stopping sing-box...");
        stop_sing_internal().await;
        return Err("sing-box started but connectivity check failed".into());
    }

    Ok(())
}

async fn stop_sing_internal() {
    let mut lock = SING_PROCESS.lock().await;
    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait().ok().flatten().is_none() {
            // Use SIGTERM to allow sing-box to gracefully shutdown and cleanup nftables rules
            if let Some(pid) = proc.child.id() {
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                // Wait up to 3 seconds for graceful shutdown
                for _ in 0..30 {
                    sleep(Duration::from_millis(100)).await;
                    if proc.child.try_wait().ok().flatten().is_some() {
                        break;
                    }
                }
                // Force kill if still running
                if proc.child.try_wait().ok().flatten().is_none() {
                    proc.child.start_kill().ok();
                    let _ = proc.child.wait().await;
                }
            }
        }
    }
    *lock = None;
}

async fn gen_config(
    config: &Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let my_outbounds: Vec<serde_json::Value> = config
        .nodes
        .iter()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect();
    let my_names: Vec<String> = my_outbounds
        .iter()
        .filter_map(|o| o.get("tag").and_then(|v| v.as_str()).map(String::from))
        .collect();

    let mut final_outbounds: Vec<serde_json::Value> = vec![];
    let mut final_node_names: Vec<String> = vec![];

    // Clear old status for subs that no longer exist
    {
        let mut status_map = SUB_STATUS.lock().await;
        status_map.retain(|url, _| config.subs.contains(url));
    }

    for sub in &config.subs {
        println!("Fetching subscription: {}", sub);
        let status = match fetch_sub(sub).await {
            Ok((node_names, outbounds)) => {
                let count = node_names.len();
                println!("  -> Success: fetched {} nodes", count);
                final_node_names.extend(node_names);
                final_outbounds.extend(outbounds);
                SubStatus {
                    url: sub.clone(),
                    success: count > 0,
                    node_count: count,
                    error: if count == 0 { Some("No nodes found".into()) } else { None },
                }
            }
            Err(e) => {
                eprintln!("  -> Failed to fetch subscription: {}", e);
                SubStatus {
                    url: sub.clone(),
                    success: false,
                    node_count: 0,
                    error: Some(e.to_string()),
                }
            }
        };
        SUB_STATUS.lock().await.insert(sub.clone(), status);
    }

    let total_nodes = my_outbounds.len() + final_outbounds.len();
    if total_nodes == 0 {
        return Err(
            "No nodes available: all subscriptions failed and no manual nodes configured".into(),
        );
    }

    let mut sing_box_config = get_config_template();
    if let Some(outbounds) = sing_box_config["outbounds"][0].get_mut("outbounds") {
        if let Some(arr) = outbounds.as_array_mut() {
            arr.extend(
                my_names
                    .into_iter()
                    .chain(final_node_names.into_iter())
                    .map(serde_json::Value::String),
            );
        }
    }
    if let Some(arr) = sing_box_config["outbounds"].as_array_mut() {
        arr.extend(my_outbounds.into_iter().chain(final_outbounds.into_iter()));
    }

    let sing_box_home = get_sing_box_home();
    let config_output_loc = sing_box_home.join("config.json");
    tokio::fs::write(
        &config_output_loc,
        serde_json::to_string(&sing_box_config)?,
    )
    .await?;

    println!(
        "Generated config: {}",
        serde_json::to_string(&sing_box_config).unwrap()
    );
    Ok(())
}



fn get_config_template() -> serde_json::Value {
    serde_json::json!({
        "log": {"disabled": false, "timestamp": true, "level": "info"},
        "experimental": {"clash_api": {"external_controller": "0.0.0.0:6262", "access_control_allow_origin": ["*"]}},
        "dns": {
            "final": "googledns",
            "strategy": "ipv4_only",
            "disable_cache": false,
            "independent_cache": true,
            "servers": [
                {"type": "udp", "tag": "googledns", "server": "8.8.8.8", "detour": "proxy"},
                {"tag": "local", "type": "udp", "server": "223.5.5.5"}
            ],
            "rules": [{"rule_set": ["chinasite"], "action": "route", "server": "local"}]
        },
        "inbounds": [
            {"type": "tun", "tag": "tun-in", "interface_name": "sing-tun", "address": ["172.18.0.1/30"], "mtu": 9000, "auto_route": true, "strict_route": true, "auto_redirect": true}
        ],
        "outbounds": [
            {"type": "selector", "tag": "proxy", "outbounds": []},
            {"type": "direct", "tag": "direct"}
        ],
        "route": {
            "final": "proxy",
            "auto_detect_interface": true,
            "default_domain_resolver": "local",
            "rules": [
                {"action": "sniff"},
                {"protocol": "dns", "action": "hijack-dns"},
                {"ip_is_private": true, "rule_set": ["chinaip", "chinasite"], "action": "route", "outbound": "direct"}
            ],
            "rule_set": [
                {"type": "local", "tag": "chinasite", "format": "binary", "path": "./chinasite.srs"},
                {"type": "local", "tag": "chinaip", "format": "binary", "path": "./chinaip.srs"}
            ]
        }
    })
}

async fn fetch_sub(
    link: &str,
) -> Result<(Vec<String>, Vec<serde_json::Value>), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let res = client
        .get(link)
        .header("User-Agent", "clash-meta")
        .send()
        .await?;
    let text = res.text().await?;
    let clash_obj: serde_yaml::Value = serde_yaml::from_str(&text)?;
    let proxies = clash_obj
        .get("proxies")
        .and_then(|p| p.as_sequence())
        .unwrap_or(&vec![])
        .clone();

    let nodes: Vec<serde_yaml::Value> = proxies.into_iter().collect();
    let mut node_names = vec![];
    let mut outbounds = vec![];

    for node in nodes {
        let typ = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
        match typ {
            "hysteria2" => {
                let hysteria2 = Hysteria2 {
                    outbound_type: "hysteria2".to_string(),
                    tag: name.to_string(),
                    server: node
                        .get("server")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    server_port: node.get("port").and_then(|p| p.as_u64()).unwrap_or(0) as u16,
                    password: node
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string(),
                    up_mbps: 40,
                    down_mbps: 350,
                    tls: Tls {
                        enabled: true,
                        server_name: node
                            .get("sni")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string()),
                        insecure: true,
                    },
                };
                node_names.push(name.to_string());
                outbounds.push(serde_json::to_value(hysteria2)?);
            }
            "anytls" => {
                let anytls = AnyTls {
                    outbound_type: "anytls".to_string(),
                    tag: name.to_string(),
                    server: node
                        .get("server")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    server_port: node.get("port").and_then(|p| p.as_u64()).unwrap_or(0) as u16,
                    password: node
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string(),
                    tls: Tls {
                        enabled: true,
                        server_name: node
                            .get("sni")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string()),
                        insecure: node
                            .get("skip-cert-verify")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                    },
                };
                node_names.push(name.to_string());
                outbounds.push(serde_json::to_value(anytls)?);
            }
            "ss" => {
                let ss = Shadowsocks {
                    outbound_type: "shadowsocks".to_string(),
                    tag: name.to_string(),
                    server: node
                        .get("server")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string(),
                    server_port: node.get("port").and_then(|p| p.as_u64()).unwrap_or(0) as u16,
                    method: node
                        .get("cipher")
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string(),
                    password: node
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string(),
                };
                node_names.push(name.to_string());
                outbounds.push(serde_json::to_value(ss)?);
            }
            _ => {}
        }
    }
    Ok((node_names, outbounds))
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Check for root privileges
    if !Uid::effective().is_root() {
        eprintln!("Error: This application must be run as root.");
        std::process::exit(1);
    }

    // Clean up backup file from previous upgrade
    if let Ok(current_exe) = std::env::current_exe() {
        let backup_path = format!("{}.bak", current_exe.display());
        if std::path::Path::new(&backup_path).exists() {
            let _ = fs::remove_file(&backup_path);
        }
    }

    println!("Reading configuration...");
    let config: Config = serde_yaml::from_str(&tokio::fs::read_to_string("config.yaml").await?)?;
    let port = config.port.unwrap_or(DEFAULT_PORT);

    // Extract embedded sing-box binary and determine working directory
    let _ = extract_sing_box()?;
    // sing_box_home is hardcoded to /tmp/miao-sing-box inside functions

    // Generate initial config, retrying until success
    println!("Generating initial config...");
    loop {
        match gen_config(&config).await {
            Ok(_) => break,
            Err(e) => {
                eprintln!(
                    "Failed to generate config: {}. Retrying in 300 seconds...",
                    e
                );
                sleep(Duration::from_secs(300)).await;
            }
        }
    }

    // Check OpenWrt dependencies
    println!("Checking dependencies...");
    if let Err(e) = check_and_install_openwrt_dependencies().await {
        eprintln!("Failed to check or install OpenWrt dependencies: {}", e);
    }

    // Start sing-box
    match start_sing_internal().await {
        Ok(_) => {
            println!("sing-box started successfully");
            // Try to restore last selected proxy (non-blocking)
            tokio::spawn(async {
                restore_last_proxy().await;
            });
        }
        Err(e) => eprintln!("Failed to start sing-box: {}", e),
    }

    let app_state = Arc::new(AppState {
        config: Mutex::new(config.clone()),
    });



    // Build router with API endpoints
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/favicon.svg", get(serve_favicon))
        // Static assets
        .route("/assets/vue.js", get(serve_vue))
        .route("/assets/bootstrap-icons.css", get(serve_bootstrap_css))
        .route("/assets/fonts/bootstrap-icons.woff2", get(serve_font_woff2))
        .route("/assets/fonts/bootstrap-icons.woff", get(serve_font_woff))
        // Status and service control
        .route("/api/status", get(get_status))
        .route("/api/service/start", post(start_service))
        .route("/api/service/stop", post(stop_service))
        // Connectivity test
        .route("/api/connectivity", post(test_connectivity))
        // Version and upgrade
        .route("/api/version", get(get_version))
        .route("/api/upgrade", post(upgrade))
        // Subscription management
        .route("/api/subs", get(get_subs))
        .route("/api/subs", post(add_sub))
        .route("/api/subs", delete(delete_sub))
        .route("/api/subs/refresh", post(refresh_subs))
        // Node management
        .route("/api/nodes", get(get_nodes))
        .route("/api/nodes", post(add_node))
        .route("/api/nodes", delete(delete_node))
        // Last proxy (for auto-restore)
        .route("/api/last-proxy", post(set_last_proxy))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    println!("✅ Miao 控制面板已启动: http://localhost:{}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
