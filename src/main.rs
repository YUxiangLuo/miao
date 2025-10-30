use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, Json},
    routing::{get, post},
    Router,
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

async fn serve_index() -> Html<&'static str> {
    Html(include_str!("../public/index.html"))
}

#[derive(Clone, Deserialize)]
struct Config {
    port: u16,
    sing_box_home: String,
    subs: Vec<String>,
    nodes: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct Anytls {
    #[serde(rename = "type")]
    outbound_type: String,
    tag: String,
    server: String,
    server_port: u16,
    password: String,
    tls: Tls,
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

lazy_static! {
    static ref SING_PROCESS: Mutex<Option<tokio::process::Child>> = Mutex::new(None);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config: Config = serde_yaml::from_str(&tokio::fs::read_to_string("miao.yaml").await?)?;
    let port = config.port;
    let sing_box_home = config.sing_box_home.clone();

    // Generate initial config
    gen_config(&config).await?;

    // Start sing if possible
    let _ = start_sing(&sing_box_home).await;

    let app_state = Arc::new(config);
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/config", get(get_config_handler))
        .route("/api/config/generate", get(generate_config_handler))
        .route("/api/sing/restart", post(restart_sing))
        .route("/api/sing/start", post(start_sing_handler))
        .route("/api/sing/stop", post(stop_sing_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn get_config_handler(
    State(config): State<Arc<Config>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let config_output_loc = format!("{}/config.json", config.sing_box_home);
    let stat = tokio::fs::metadata(&config_output_loc)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "config file not found".to_string()))?;
    let config_content = tokio::fs::read_to_string(&config_output_loc)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "config file not found".to_string()))?;
    let config_json: serde_json::Value = serde_json::from_str(&config_content)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "config_stat": serde_json::json!({
            "size": stat.len(),
            "modified": stat.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH).duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
            "created": stat.created().unwrap_or(std::time::SystemTime::UNIX_EPOCH).duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
        }),
        "config_content": serde_json::to_string_pretty(&config_json).unwrap()
    })))
}

async fn generate_config_handler(
    State(config): State<Arc<Config>>,
) -> Result<axum::response::Response, (StatusCode, String)> {
    match gen_config(&*config).await {
        Ok(_) => {
            let config_output_loc = format!("{}/config.json", config.sing_box_home);
            let file = tokio::fs::read(&config_output_loc)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Ok(axum::response::Response::new(axum::body::Body::from(file)))
        }
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn gen_config(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        let (node_names, outbounds) = fetch_sub(sub).await?;
        final_node_names.extend(node_names);
        final_outbounds.extend(outbounds);
    }
    let mut sing_box_config = get_config_template();
    if let Some(outbounds) = sing_box_config["outbounds"][0].get_mut("outbounds") {
        if let Some(arr) = outbounds.as_array_mut() {
            arr.extend(
                my_names
                    .into_iter()
                    .chain(final_node_names.into_iter())
                    .map(|s| serde_json::Value::String(s)),
            );
        }
    }
    if let Some(arr) = sing_box_config["outbounds"].as_array_mut() {
        arr.extend(my_outbounds.into_iter().chain(final_outbounds.into_iter()));
    }
    let config_output_loc = format!("{}/config.json", config.sing_box_home);
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
        "experimental": {"clash_api": {"external_controller": "0.0.0.0:9090", "external_ui": "dashboard"}},
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
            {"type": "urltest", "tag": "proxy", "outbounds": []},
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
    let client = reqwest::Client::new();
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
    let nodes: Vec<serde_yaml::Value> = proxies
        .into_iter()
        .filter(|p| {
            p.get("name")
                .and_then(|n| n.as_str())
                .map(|n| {
                    n.contains("JP")
                        || n.contains("日本")
                        || n.contains("SG")
                        || n.contains("新加坡")
                        || n.contains("TW")
                        || n.contains("台湾")
                })
                .unwrap_or(false)
        })
        .collect();
    let mut node_names = vec![];
    let mut outbounds = vec![];
    for node in nodes {
        let typ = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
        match typ {
            "anytls" => {
                let anytls = Anytls {
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
                            .and_then(|s| s.as_bool())
                            .unwrap_or(false),
                    },
                };
                node_names.push(name.to_string());
                outbounds.push(serde_json::to_value(anytls)?);
            }
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

async fn restart_sing(State(config): State<Arc<Config>>) -> Result<String, (StatusCode, String)> {
    stop_sing_internal().await;
    match start_sing(&config.sing_box_home).await {
        Ok(_) => Ok("ok".to_string()),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn start_sing_handler(
    State(config): State<Arc<Config>>,
) -> Result<String, (StatusCode, String)> {
    let mut lock = SING_PROCESS.lock().await;
    if lock.is_some() && lock.as_mut().unwrap().try_wait().unwrap().is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "sing box is already running".to_string(),
        ));
    }
    drop(lock);
    match start_sing(&config.sing_box_home).await {
        Ok(_) => Ok("ok".to_string()),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

async fn stop_sing_handler(
    State(_config): State<Arc<Config>>,
) -> Result<String, (StatusCode, String)> {
    stop_sing_internal().await;
    Ok("stopped".to_string())
}

async fn start_sing(sing_box_home: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut lock = SING_PROCESS.lock().await;
    if lock.is_some() && lock.as_mut().unwrap().try_wait()?.is_none() {
        return Err("already running!".into());
    }
    let child = tokio::process::Command::new("sing-box")
        .current_dir(sing_box_home)
        .arg("run")
        .arg("-c")
        .arg("config.json")
        .env(
            "PATH",
            format!("{}:{}", std::env::var("PATH")?, sing_box_home),
        )
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    *lock = Some(child);
    Ok(())
}

async fn stop_sing_internal() {
    let mut lock = SING_PROCESS.lock().await;
    if let Some(ref mut p) = *lock {
        if p.try_wait().ok().flatten().is_none() {
            p.start_kill().ok();
        }
    }
    *lock = None;
}
