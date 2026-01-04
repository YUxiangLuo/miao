use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::{delete, get, post},
    Router,
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

// Embed sing-box binary based on target architecture
#[cfg(target_arch = "x86_64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../embedded/sing-box-amd64");

#[cfg(target_arch = "aarch64")]
const SING_BOX_BINARY: &[u8] = include_bytes!("../embedded/sing-box-arm64");

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Clone, Serialize, Deserialize)]
struct Config {
    port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sing_box_home: Option<String>,
    #[serde(default)]
    subs: Vec<String>,
    #[serde(default)]
    nodes: Vec<String>,
}

struct AppState {
    config: Mutex<Config>,
    sing_box_home: String,
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



// Request types for subscription and node management
#[derive(Deserialize)]
struct SubRequest {
    url: String,
}

#[derive(Deserialize)]
struct NodeRequest {
    tag: String,
    server: String,
    server_port: u16,
    password: String,
    #[serde(default)]
    sni: Option<String>,
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

lazy_static! {
    static ref SING_PROCESS: Mutex<Option<SingBoxProcess>> = Mutex::new(None);
}

// ============================================================================
// API Handlers
// ============================================================================

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../public/index.html"))
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
    State(state): State<Arc<AppState>>,
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

    match start_sing_internal(&state.sing_box_home).await {
        Ok(_) => Ok(Json(ApiResponse::success_no_data("sing-box started successfully"))),
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

/// POST /api/service/restart - Restart sing-box
async fn restart_service(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    stop_sing_internal().await;
    sleep(Duration::from_millis(500)).await;

    match start_sing_internal(&state.sing_box_home).await {
        Ok(_) => Ok(Json(ApiResponse::success_no_data("sing-box restarted successfully"))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::error(format!("Failed to restart: {}", e))),
        )),
    }
}



// ============================================================================
// Subscription Management APIs
// ============================================================================

/// GET /api/subs - Get all subscription URLs
async fn get_subs(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<String>>> {
    let config = state.config.lock().await;
    Json(ApiResponse::success("Subscriptions loaded", config.subs.clone()))
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

    // Regenerate and restart in background
    let sing_box_home = state.sing_box_home.clone();
    tokio::spawn(async move {
        regenerate_and_restart(&config_clone, &sing_box_home).await;
    });

    Ok(Json(ApiResponse::success_no_data("Subscription added, restarting...")))
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

    let sing_box_home = state.sing_box_home.clone();
    tokio::spawn(async move {
        regenerate_and_restart(&config_clone, &sing_box_home).await;
    });

    Ok(Json(ApiResponse::success_no_data("Subscription deleted, restarting...")))
}

/// POST /api/subs/refresh - Refresh subscriptions and restart
async fn refresh_subs(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ApiResponse<()>>, (StatusCode, Json<ApiResponse<()>>)> {
    let config = state.config.lock().await;
    let config_clone = config.clone();
    drop(config);

    let sing_box_home = state.sing_box_home.clone();
    tokio::spawn(async move {
        regenerate_and_restart(&config_clone, &sing_box_home).await;
    });

    Ok(Json(ApiResponse::success_no_data("Subscriptions refreshed, restarting...")))
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

        // Build Hysteria2 node
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

        let node_json = serde_json::to_string(&node).map_err(|e| {
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

    let sing_box_home = state.sing_box_home.clone();
    tokio::spawn(async move {
        regenerate_and_restart(&config_clone, &sing_box_home).await;
    });

    Ok(Json(ApiResponse::success_no_data("Node added, restarting...")))
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

    let sing_box_home = state.sing_box_home.clone();
    tokio::spawn(async move {
        regenerate_and_restart(&config_clone, &sing_box_home).await;
    });

    Ok(Json(ApiResponse::success_no_data("Node deleted, restarting...")))
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
async fn regenerate_and_restart(config: &Config, sing_box_home: &str) {
    // Regenerate config
    if let Err(e) = gen_config(config, sing_box_home).await {
        eprintln!("Failed to regenerate config: {}", e);
        return;
    }
    println!("Config regenerated successfully");

    // Stop and restart sing-box
    stop_sing_internal().await;
    sleep(Duration::from_millis(500)).await;

    if let Err(e) = start_sing_internal(sing_box_home).await {
        eprintln!("Failed to restart sing-box: {}", e);
    } else {
        println!("sing-box restarted successfully");
    }
}

/// Extract embedded sing-box binary to current working directory
fn extract_sing_box() -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    let current_dir = std::env::current_dir()?;
    let sing_box_path = current_dir.join("sing-box");

    if !sing_box_path.exists() {
        println!("Extracting embedded sing-box binary to {:?}", sing_box_path);
        fs::write(&sing_box_path, SING_BOX_BINARY)?;
        fs::set_permissions(&sing_box_path, fs::Permissions::from_mode(0o755))?;
        println!("sing-box binary extracted successfully");
    }

    let dashboard_dir = current_dir.join("dashboard");
    if !dashboard_dir.exists() {
        fs::create_dir_all(&dashboard_dir)?;
    }

    Ok(current_dir)
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
    sing_box_home: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut lock = SING_PROCESS.lock().await;
    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait()?.is_none() {
            return Err("already running!".into());
        }
    }

    let sing_box_path = PathBuf::from(sing_box_home).join("sing-box");
    let config_path = PathBuf::from(sing_box_home).join("config.json");

    println!("Starting sing-box from: {:?}", sing_box_path);
    println!("Using config: {:?}", config_path);

    let mut child = tokio::process::Command::new(&sing_box_path)
        .current_dir(sing_box_home)
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
        // Stop sing-box and clean up
        println!("Connectivity check failed, stopping sing-box...");
        let mut lock = SING_PROCESS.lock().await;
        if let Some(ref mut proc) = *lock {
            proc.child.start_kill().ok();
            // Wait for process to exit
            let _ = proc.child.wait().await;
        }
        *lock = None;
        return Err("sing-box started but connectivity check failed".into());
    }

    Ok(())
}

async fn stop_sing_internal() {
    let mut lock = SING_PROCESS.lock().await;
    if let Some(ref mut proc) = *lock {
        if proc.child.try_wait().ok().flatten().is_none() {
            proc.child.start_kill().ok();
        }
    }
    *lock = None;
}

async fn gen_config(
    config: &Config,
    sing_box_home: &str,
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

    for sub in &config.subs {
        match fetch_sub(sub).await {
            Ok((node_names, outbounds)) => {
                final_node_names.extend(node_names);
                final_outbounds.extend(outbounds);
            }
            Err(e) => {
                eprintln!("Failed to fetch subscription from {}: {}", sub, e);
            }
        }
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

    let config_output_loc = format!("{}/config.json", sing_box_home);
    tokio::fs::write(
        &config_output_loc,
        serde_json::to_string_pretty(&sing_box_config)?,
    )
    .await?;

    println!(
        "Generated config: {}",
        serde_json::to_string_pretty(&sing_box_config).unwrap()
    );
    Ok(())
}



fn get_config_template() -> serde_json::Value {
    serde_json::json!({
        "log": {"disabled": false, "timestamp": true, "level": "info"},
        "experimental": {"clash_api": {"external_controller": "0.0.0.0:6262", "access_control_allow_origin": ["*"]}},
        "dns": {
            "final": "googledns",
            "strategy": "prefer_ipv4",
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
                {"ip_is_private": true, "action": "route", "outbound": "direct"},
                {"protocol": ["bittorrent"], "action": "route", "outbound": "direct"},
                {"rule_set": ["chinasite"], "action": "route", "outbound": "direct"},
                {"rule_set": ["chinaip"], "action": "route", "outbound": "direct"}
            ],
            "rule_set": [
                {"type": "remote", "tag": "chinasite", "format": "binary", "url": "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-geolocation-cn.srs"},
                {"type": "remote", "tag": "chinaip", "format": "binary", "url": "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs"}
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
    let config: Config = serde_yaml::from_str(&tokio::fs::read_to_string("config.yaml").await?)?;
    let port = config.port;

    // Extract embedded sing-box binary and determine working directory
    let sing_box_home = if let Some(custom_home) = &config.sing_box_home {
        custom_home.clone()
    } else {
        extract_sing_box()?.to_string_lossy().to_string()
    };

    // Generate initial config, retrying until success
    loop {
        match gen_config(&config, &sing_box_home).await {
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
    if let Err(e) = check_and_install_openwrt_dependencies().await {
        eprintln!("Failed to check or install OpenWrt dependencies: {}", e);
    }

    // Start sing-box
    match start_sing_internal(&sing_box_home).await {
        Ok(_) => println!("sing-box started successfully"),
        Err(e) => eprintln!("Failed to start sing-box: {}", e),
    }

    let app_state = Arc::new(AppState {
        config: Mutex::new(config.clone()),
        sing_box_home: sing_box_home.clone(),
    });



    // Build router with API endpoints
    let app = Router::new()
        .route("/", get(serve_index))
        // Status and service control
        .route("/api/status", get(get_status))
        .route("/api/service/start", post(start_service))
        .route("/api/service/stop", post(stop_service))
        .route("/api/service/restart", post(restart_service))

        // Subscription management
        .route("/api/subs", get(get_subs))
        .route("/api/subs", post(add_sub))
        .route("/api/subs", delete(delete_sub))
        .route("/api/subs/refresh", post(refresh_subs))
        // Node management
        .route("/api/nodes", get(get_nodes))
        .route("/api/nodes", post(add_node))
        .route("/api/nodes", delete(delete_node))
        .with_state(app_state);

    println!("Miao server listening on http://0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
