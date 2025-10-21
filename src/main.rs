use axum::extract::State;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Child;
use tokio::sync::RwLock;

#[derive(Parser)]
struct Args {
    /// Path to the config file (default: miao.yaml)
    #[arg(short, long, default_value = "miao.yaml")]
    config: String,
    /// Port to listen on (default: 6161)
    #[arg(short, long, default_value = "6161")]
    port: u16,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Deserialize)]
struct Config {
    subs: Vec<String>,
    sing_box_home: Option<String>,
    nodes: Option<Vec<String>>,
    rules: Option<Rules>,
    port: Option<u16>,
}

#[derive(Clone, Deserialize)]
struct Rules {
    direct_txt: String,
}

#[derive(Clone)]
struct AppState {
    config: Config,
    sing_box_process: Arc<RwLock<Option<Child>>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let content = tokio::fs::read_to_string(&args.config).await?;
    let mut config: Config = serde_yaml::from_str(&content)?;
    let port = config.port.unwrap_or(args.port);
    config.port = Some(port);

    let state = AppState {
        config: config.clone(),
        sing_box_process: Arc::new(RwLock::new(None)),
    };

    // Generate config and start sing-box at startup
    println!("Generating config...");
    if let Err(e) = gen_config(&config).await {
        eprintln!("Failed to generate config: {}", e);
        std::process::exit(1);
    }
    println!("Starting sing-box...");
    if let Err(e) = start_sing_internal(&state).await {
        eprintln!("Failed to start sing-box: {}", e);
        std::process::exit(1);
    }
    println!("Sing-box started successfully.");

    let app = Router::new()
        .route("/api/rule/generate", post(rule_generate))
        .route("/api/config", get(get_config))
        .route("/api/config/generate", post(generate_config))
        .route("/api/sing/log-live", get(log_live))
        .route("/api/sing/restart", post(sing_restart))
        .route("/api/sing/start", post(sing_start))
        .route("/api/sing/stop", post(sing_stop))
        .route("/api/net-checks/manual", get(net_check_manual))
        .with_state(Arc::new(state));

    let addr = format!("0.0.0.0:{}", port);
    println!("Starting server on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn rule_generate(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match gen_rule(&state.config).await {
        Ok(stat) => (StatusCode::OK, Json(stat)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn get_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let home = state
        .config
        .sing_box_home
        .as_ref()
        .map_or(".", |s| s.as_str());
    let loc = format!("{}/config.json", home);
    match tokio::fs::metadata(&loc).await {
        Ok(stat) => match tokio::fs::read_to_string(&loc).await {
            Ok(content) => {
                let stat_json = json!({
                    "size": stat.len(),
                    "modified": stat.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH).duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs()
                });
                let content_value: serde_json::Value =
                    serde_json::from_str(&content).unwrap_or(json!({}));
                (
                    StatusCode::OK,
                    Json(json!({
                        "config_stat": stat_json,
                        "config_content": content_value
                    })),
                )
            }
            Err(_) => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "config file not found" })),
            ),
        },
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "config file not found" })),
        ),
    }
}

async fn generate_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match gen_config(&state.config).await {
        Ok(_) => {
            let home = state
                .config
                .sing_box_home
                .as_ref()
                .map_or(".", |s| s.as_str());
            let loc = format!("{}/config.json", home);
            match tokio::fs::read_to_string(&loc).await {
                Ok(content) => (StatusCode::OK, Json(json!({ "config": content }))),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                ),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn log_live(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let home = state
        .config
        .sing_box_home
        .as_ref()
        .map_or(".", |s| s.as_str());
    let log_path = format!("{}/box.log", home);
    match tokio::process::Command::new("tail")
        .args(&["-n", "50", &log_path])
        .output()
        .await
    {
        Ok(output) => (
            StatusCode::OK,
            Json(json!(String::from_utf8_lossy(&output.stdout))),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn sing_restart(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    stop_sing_internal(&state).await;
    match start_sing_internal(&state).await {
        Ok(_) => (StatusCode::OK, Json(json!("ok"))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn sing_start(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let running = state.sing_box_process.read().await.is_some();
    if running {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "sing box is already running" })),
        );
    }
    match start_sing_internal(&state).await {
        Ok(_) => (StatusCode::OK, Json(json!("ok"))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}

async fn sing_stop(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    stop_sing_internal(&state).await;
    (StatusCode::OK, Json(json!("stopped")))
}

async fn net_check_manual(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
    match check_connection().await {
        Ok(_) => (StatusCode::OK, Json(json!("ok"))),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!("not ok"))),
    }
}

async fn stop_sing_internal(state: &AppState) {
    let mut proc = state.sing_box_process.write().await;
    if let Some(ref mut child) = *proc {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    *proc = None;
}

async fn start_sing_internal(state: &AppState) -> Result<()> {
    let mut proc = state.sing_box_process.write().await;
    if proc.is_some() {
        return Err("already running".into());
    }

    let home = state
        .config
        .sing_box_home
        .as_ref()
        .map_or(".", |s| s.as_str());
    let path = format!("{}/sing-box", home);
    let mut child = tokio::process::Command::new(&path)
        .args(&["run", "-c", "config.json"])
        .current_dir(home)
        .env(
            "PATH",
            format!("{}:{}", home, std::env::var("PATH").unwrap_or_default()),
        )
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;

    tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;
    match child.try_wait() {
        Ok(Some(_)) => return Err("sing box failed to start".into()),
        Ok(None) => {
            *proc = Some(child);
            if check_connection().await.is_ok() {
                Ok(())
            } else {
                stop_sing_internal(state).await;
                Err("sing box started but failed to connect to internet".into())
            }
        }
        Err(e) => Err(e.into()),
    }
}

async fn check_connection() -> Result<()> {
    let client = reqwest::Client::new();
    let res = client
        .get("https://gstatic.com/generate_204")
        .send()
        .await?;
    if res.status().as_u16() == 204 {
        Ok(())
    } else {
        Err("Connection check failed".into())
    }
}

async fn gen_config(config: &Config) -> Result<()> {
    let mut my_outbounds: Vec<serde_json::Value> = vec![];
    if let Some(nodes) = &config.nodes {
        for node in nodes {
            let outbound: serde_json::Value = serde_json::from_str(node)?;
            my_outbounds.push(outbound);
        }
    }
    println!("Custom outbounds parsed: {}", my_outbounds.len());

    let mut final_outbounds: Vec<serde_json::Value> = vec![];
    let mut final_node_names: Vec<String> = vec![];
    for sub in &config.subs {
        let (node_names, outbounds) = fetch_sub(sub).await?;
        final_node_names.extend(node_names);
        final_outbounds.extend(outbounds);
    }
    println!("Fetched {} nodes from subscriptions", final_outbounds.len());

    let mut sing_box_config = get_sing_box_config();
    if let Some(proxy_outbound) = sing_box_config.outbounds.get_mut(0) {
        if let Some(outbounds_array) = proxy_outbound
            .get_mut("outbounds")
            .and_then(|v| v.as_array_mut())
        {
            let my_names: Vec<serde_json::Value> = my_outbounds
                .iter()
                .filter_map(|v| get_outbound_tag(v))
                .map(|tag| json!(tag))
                .collect();
            outbounds_array.extend(my_names);
            let final_names: Vec<serde_json::Value> =
                final_node_names.iter().map(|name| json!(name)).collect();
            outbounds_array.extend(final_names);
        }
    }

    sing_box_config.outbounds.extend(my_outbounds);
    sing_box_config.outbounds.extend(final_outbounds);

    let home = config.sing_box_home.as_ref().map_or(".", |s| s.as_str());
    let loc = format!("{}/config.json", home);
    tokio::fs::create_dir_all(
        std::path::Path::new(&loc)
            .parent()
            .unwrap_or(std::path::Path::new(".")),
    )
    .await?;
    let content = serde_json::to_string_pretty(&sing_box_config)?;
    tokio::fs::write(&loc, content).await?;
    println!("Config file written to {}", loc);
    Ok(())
}

async fn gen_rule(config: &Config) -> Result<serde_json::Value> {
    let rules = config.rules.as_ref().ok_or("rules not configured")?;
    let client = reqwest::Client::new();
    let res = client.get(&rules.direct_txt).send().await?;
    let text = res.text().await?;
    tokio::fs::write("direct.txt", &text).await?;
    let direct_items: Vec<&str> = text.lines().collect();

    #[derive(Serialize)]
    struct DomainSet {
        rules: Vec<DomainRules>,
        version: u32,
    }

    #[derive(Serialize)]
    struct DomainRules {
        domain: Vec<String>,
        domain_suffix: Vec<String>,
        domain_regex: Vec<String>,
    }

    let mut domain_set = DomainSet {
        rules: vec![DomainRules {
            domain: vec![],
            domain_suffix: vec![],
            domain_regex: vec![],
        }],
        version: 3,
    };
    for item in direct_items {
        if item.starts_with("full:") {
            domain_set.rules[0].domain.push(item.replace("full:", ""));
        } else if item.starts_with("regexp:") {
            domain_set.rules[0]
                .domain_regex
                .push(item.replace("regexp:", ""));
        } else if !item.is_empty() {
            domain_set.rules[0].domain_suffix.push(item.to_string());
        }
    }
    let json_content = serde_json::to_string(&domain_set)?;
    tokio::fs::write("direct.json", json_content).await?;
    let home = config.sing_box_home.as_ref().map_or(".", |s| s.as_str());
    if std::fs::metadata(format!("{}/chinasite.srs", home)).is_ok() {
        tokio::fs::copy(
            format!("{}/chinasite.srs", home),
            format!("{}/chinasite.srs.bak", home),
        )
        .await?;
    }
    let mut cmd = tokio::process::Command::new(format!("{}/sing-box", home))
        .args(&[
            "rule-set",
            "compile",
            "--output",
            &format!("{}/chinasite.srs", home),
            "direct.json",
        ])
        .current_dir(home)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    let status = cmd.wait().await?;
    if status.success() {
        let metadata = tokio::fs::metadata(format!("{}/chinasite.srs", home)).await?;
        Ok(json!({
            "size": metadata.len(),
            "modified": metadata.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH).duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs()
        }))
    } else {
        if std::fs::metadata(format!("{}/chinasite.srs.bak", home)).is_ok() {
            tokio::fs::copy(
                format!("{}/chinasite.srs.bak", home),
                format!("{}/chinasite.srs", home),
            )
            .await?;
        }
        Err("Failed to compile rule set".into())
    }
}

async fn fetch_sub(link: &str) -> Result<(Vec<String>, Vec<serde_json::Value>)> {
    let client = reqwest::Client::new();
    let res = client
        .get(link)
        .header("User-Agent", "clash-meta")
        .send()
        .await?;
    let text = res.text().await?;
    let clash: ClashConfig = serde_yaml::from_str(&text)?;
    println!("Fetched {} proxies from {}", clash.proxies.len(), link);
    let nodes: Vec<ClashProxy> = clash
        .proxies
        .into_iter()
        .filter(|p| p.name.contains("JP") || p.name.contains("TW") || p.name.contains("SG"))
        .collect();
    println!("Filtered {} nodes (JP/TW/SG)", nodes.len());

    let mut node_names = vec![];
    let mut outbounds: Vec<serde_json::Value> = vec![];
    for node in nodes {
        match node.proxy_type.as_str() {
            "anytls" => {
                println!("Converting anytls node: {}", node.name);
                let outbound = json!({
                    "type": "anytls",
                    "tag": node.name,
                    "server": node.server,
                    "server_port": node.server_port,
                    "password": node.password,
                    "tls": {
                        "enabled": true,
                        "server_name": node.sni,
                        "insecure": node.skip_cert_verify.unwrap_or(true)
                    }
                });
                node_names.push(node.name);
                outbounds.push(outbound);
            }
            "hysteria2" => {
                println!("Converting hysteria2 node: {}", node.name);
                let outbound = json!({
                    "type": "hysteria2",
                    "tag": node.name,
                    "server": node.server,
                    "server_port": node.server_port,
                    "password": node.password,
                    "up_mbps": 40,
                    "down_mbps": 350,
                    "tls": {
                        "enabled": true,
                        "server_name": node.sni,
                        "insecure": true
                    }
                });
                node_names.push(node.name);
                outbounds.push(outbound);
            }
            _ => {
                println!("Skipping node: {} ({})", node.name, node.proxy_type);
            }
        }
    }
    Ok((node_names, outbounds))
}

fn get_sing_box_config() -> SingBoxConfig {
    SingBoxConfig {
        log: Log {
            disabled: true,
            output: "./box.log".to_string(),
            timestamp: true,
            level: "info".to_string(),
        },
        experimental: Experimental {
            clash_api: ClashApi {
                external_controller: "0.0.0.0:9090".to_string(),
                external_ui: "dashboard".to_string(),
            },
        },
        dns: Dns {
            r#final: "googledns".to_string(),
            strategy: "prefer_ipv4".to_string(),
            independent_cache: true,
            servers: vec![
                DnsServer {
                    _type: "udp".to_string(),
                    tag: "googledns".to_string(),
                    server: "8.8.8.8".to_string(),
                    detour: Some("proxy".to_string()),
                },
                DnsServer {
                    _type: "udp".to_string(),
                    tag: "local".to_string(),
                    server: "223.5.5.5".to_string(),
                    detour: None,
                },
            ],
            rules: vec![DnsRule {
                rule_set: vec!["chinasite".to_string()],
                action: "route".to_string(),
                server: Some("local".to_string()),
            }],
        },
        inbounds: vec![Inbound {
            _type: "tun".to_string(),
            tag: "tun-in".to_string(),
            interface_name: "sing-tun".to_string(),
            address: vec!["172.18.0.1/30".to_string()],
            mtu: 9000,
            auto_route: true,
            strict_route: true,
            auto_redirect: true,
        }],
        outbounds: vec![
            serde_json::json!({
                "type": "urltest",
                "tag": "proxy",
                "outbounds": []
            }),
            serde_json::json!({
                "type": "direct",
                "tag": "direct"
            }),
        ],
        route: Route {
            r#final: "proxy".to_string(),
            auto_detect_interface: true,
            default_domain_resolver: "local".to_string(),
            rules: vec![
                RouteRule {
                    action: "sniff".to_string(),
                    ..Default::default()
                },
                RouteRule {
                    protocol: Some("dns".to_string()),
                    action: "hijack-dns".to_string(),
                    ..Default::default()
                },
                RouteRule {
                    ip_is_private: Some(true),
                    action: "route".to_string(),
                    outbound: Some("direct".to_string()),
                    ..Default::default()
                },
                RouteRule {
                    process_path: Some(vec![
                        "/usr/bin/qbittorrent".to_string(),
                        "/usr/bin/NetworkManager".to_string(),
                    ]),
                    action: "route".to_string(),
                    outbound: Some("direct".to_string()),
                    ..Default::default()
                },
                RouteRule {
                    rule_set: Some(vec!["chinasite".to_string()]),
                    action: "route".to_string(),
                    outbound: Some("direct".to_string()),
                    ..Default::default()
                },
            ],
            rule_set: vec![RuleSet {
                _type: "local".to_string(),
                tag: "chinasite".to_string(),
                format: "binary".to_string(),
                path: "chinasite.srs".to_string(),
            }],
        },
    }
}

fn get_outbound_tag(outbound: &serde_json::Value) -> Option<&str> {
    outbound.get("tag").and_then(|v| v.as_str())
}

#[derive(Serialize)]
struct SingBoxConfig {
    log: Log,
    experimental: Experimental,
    dns: Dns,
    inbounds: Vec<Inbound>,
    outbounds: Vec<serde_json::Value>,
    route: Route,
}

#[derive(Serialize)]
struct Log {
    disabled: bool,
    output: String,
    timestamp: bool,
    level: String,
}

#[derive(Serialize)]
struct Experimental {
    clash_api: ClashApi,
}

#[derive(Serialize)]
struct ClashApi {
    external_controller: String,
    external_ui: String,
}

#[derive(Serialize)]
struct Dns {
    r#final: String,
    strategy: String,
    independent_cache: bool,
    servers: Vec<DnsServer>,
    rules: Vec<DnsRule>,
}

#[derive(Serialize)]
struct DnsServer {
    #[serde(rename = "type")]
    _type: String,
    tag: String,
    server: String,
    detour: Option<String>,
}

#[derive(Serialize)]
struct DnsRule {
    rule_set: Vec<String>,
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<String>,
}

#[derive(Serialize)]
struct Inbound {
    #[serde(rename = "type")]
    _type: String,
    tag: String,
    interface_name: String,
    address: Vec<String>,
    mtu: u32,
    auto_route: bool,
    strict_route: bool,
    auto_redirect: bool,
}

#[derive(Serialize)]
struct Route {
    r#final: String,
    auto_detect_interface: bool,
    default_domain_resolver: String,
    rules: Vec<RouteRule>,
    rule_set: Vec<RuleSet>,
}

#[derive(Serialize, Default)]
struct RouteRule {
    action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip_is_private: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    process_path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule_set: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outbound: Option<String>,
}

#[derive(Serialize)]
struct RuleSet {
    #[serde(rename = "type")]
    _type: String,
    tag: String,
    format: String,
    path: String,
}

#[derive(Deserialize)]
struct ClashConfig {
    proxies: Vec<ClashProxy>,
}

#[derive(Deserialize)]
struct ClashProxy {
    #[serde(rename = "type")]
    proxy_type: String,
    name: String,
    server: Option<String>,
    #[serde(alias = "port")]
    server_port: Option<u16>,
    password: Option<String>,
    sni: Option<String>,
    #[serde(rename = "skip-cert-verify")]
    skip_cert_verify: Option<bool>,
}
