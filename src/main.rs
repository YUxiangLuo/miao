use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Parser)]
struct Args {
    /// Path to the config file (default: miao.yaml)
    #[arg(short, long, default_value = "miao.yaml")]
    config: String,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    println!("Starting config generation...");
    let content = std::fs::read_to_string(&args.config)?;
    let config: Config = serde_yaml::from_str(&content)?;
    println!("Config parsed: subs = {:?}", config.subs);
    gen_config(&config).await?;
    println!("Config generated successfully.");
    if let Err(e) = start_sing(config.sing_box_home.as_ref().unwrap_or(&".".to_string())).await {
        eprintln!("Failed to start sing-box: {}", e);
        std::process::exit(1);
    }
    println!("Sing-box started successfully.");
    Ok(())
}

#[derive(Deserialize)]
struct Config {
    subs: Vec<String>,
    sing_box_home: Option<String>,
    nodes: Option<Vec<String>>,
}

type BoxResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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

fn get_outbound_tag(outbound: &serde_json::Value) -> Option<&str> {
    outbound.get("tag").and_then(|v| v.as_str())
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

fn get_config() -> SingBoxConfig {
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

async fn gen_config(config: &Config) -> BoxResult<()> {
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

    let mut sing_box_config = get_config();
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

    let home = config
        .sing_box_home
        .as_ref()
        .unwrap_or(&".".to_string())
        .clone();
    let loc = format!("{}/config.json", home);
    std::fs::create_dir_all(std::path::Path::new(&loc).parent().unwrap())?;
    let content = serde_json::to_string_pretty(&sing_box_config)?;
    std::fs::write(&loc, content)?;
    println!("Config file written to {}", loc);
    Ok(())
}

async fn fetch_sub(link: &str) -> BoxResult<(Vec<String>, Vec<serde_json::Value>)> {
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

async fn start_sing(sing_box_home: &str) -> Result<u32> {
    let path = format!("{}/sing-box", sing_box_home);
    let mut cmd = tokio::process::Command::new(&path)
        .args(&["run", "-c", "config.json"])
        .current_dir(sing_box_home)
        .env(
            "PATH",
            format!(
                "{}:{}",
                sing_box_home,
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit()) // Inherit stderr so output is visible
        .spawn()?;

    let pid = cmd.id();

    // Wait 3 seconds
    tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;

    // Check if still running
    match cmd.try_wait() {
        Ok(Some(status)) => {
            if status.success() {
                if let Some(p) = pid {
                    std::fs::write(format!("{}/pid", sing_box_home), p.to_string())?;
                    Ok(p)
                } else {
                    Err("no pid available".into())
                }
            } else {
                Err("sing box failed to start".into())
            }
        }
        Ok(None) => {
            // Still running
            if let Some(p) = pid {
                std::fs::write(format!("{}/pid", sing_box_home), p.to_string())?;
                Ok(p)
            } else {
                Err("no pid available".into())
            }
        }
        Err(e) => Err(e.into()),
    }
}
