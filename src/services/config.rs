use tokio::time::{sleep, Duration};

use crate::error::{AppError, AppResult};
use crate::models::{Config, SubStatus};
use crate::services::{
    proxy::restore_last_proxy,
    singbox::{get_sing_box_home, start_sing_internal, stop_sing_internal},
    subscription::fetch_sub,
};
use crate::state::SUB_STATUS;

const CONFIG_CACHE_PATH: &str = "/tmp/miao-sing-box/config.json.cache";

pub async fn save_config(
    config: &Config,
) -> AppResult<()> {
    let yaml = serde_yaml::to_string(config)?;
    tokio::fs::write("config.yaml", yaml).await?;
    Ok(())
}

pub async fn save_config_cache() {
    let config_path = get_sing_box_home().join("config.json");
    if let Err(e) = tokio::fs::copy(&config_path, CONFIG_CACHE_PATH).await {
        eprintln!("Failed to save config cache: {}", e);
    } else {
        println!("Config cache saved to {}", CONFIG_CACHE_PATH);
    }
}

pub async fn restore_config_from_cache() -> AppResult<()> {
    let cache = std::path::Path::new(CONFIG_CACHE_PATH);
    if !cache.exists() {
        return Err(AppError::message("No cached config available"));
    }
    let config_path = get_sing_box_home().join("config.json");
    tokio::fs::copy(CONFIG_CACHE_PATH, &config_path)
        .await
        .map_err(|e| AppError::context("Failed to restore config from cache", e))?;
    println!("Restored config from cache");
    Ok(())
}

pub async fn regenerate_and_restart(config: &Config) -> AppResult<()> {
    let has_sub_nodes = gen_config(config)
        .await
        .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    println!("Config regenerated successfully");

    stop_sing_internal().await;
    sleep(Duration::from_millis(500)).await;

    start_sing_internal()
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box", e))?;
    println!("sing-box restarted successfully");

    if has_sub_nodes {
        save_config_cache().await;
        *crate::state::CONFIG_WARNING.lock().await = None;
    } else if !config.subs.is_empty() {
        *crate::state::CONFIG_WARNING.lock().await = Some(
            "所有订阅获取失败，请检查当前订阅".to_string()
        );
    } else {
        *crate::state::CONFIG_WARNING.lock().await = None;
    }

    tokio::spawn(async {
        restore_last_proxy().await;
    });

    Ok(())
}

/// Returns `true` if at least one subscription node was fetched successfully.
pub async fn gen_config(
    config: &Config,
) -> AppResult<bool> {
    let (my_outbounds, my_names) = collect_manual_outbounds(config);
    let mut final_outbounds: Vec<serde_json::Value> = vec![];
    let mut final_node_names: Vec<String> = vec![];

    {
        let mut status_map = SUB_STATUS.lock().await;
        status_map.retain(|url, _| config.subs.contains(url));
    }

    let sub_futures: Vec<_> = config
        .subs
        .iter()
        .map(|sub| {
            let sub = sub.clone();
            async move {
                println!("Fetching subscription: {}", sub);
                let result = tokio::time::timeout(Duration::from_secs(30), fetch_sub(&sub)).await;

                match result {
                    Ok(Ok((node_names, outbounds))) => {
                        let count = node_names.len();
                        println!("  -> Success: fetched {} nodes from {}", count, sub);
                        (sub.clone(), Ok((node_names, outbounds)))
                    }
                    Ok(Err(e)) => {
                        eprintln!("  -> Failed to fetch subscription {}: {}", sub, e);
                        (sub.clone(), Err(e.to_string()))
                    }
                    Err(_) => {
                        eprintln!("  -> Subscription {} timed out after 30s", sub);
                        (sub.clone(), Err("Request timeout".to_string()))
                    }
                }
            }
        })
        .collect();

    let results = futures::future::join_all(sub_futures).await;

    for (url, result) in results {
        let status = match result {
            Ok((node_names, outbounds)) => {
                let count = node_names.len();
                final_node_names.extend(node_names);
                final_outbounds.extend(outbounds);
                SubStatus {
                    url: url.clone(),
                    success: count > 0,
                    node_count: count,
                    error: if count == 0 {
                        Some("No nodes found".into())
                    } else {
                        None
                    },
                }
            }
            Err(e) => SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                error: Some(e),
            },
        };
        SUB_STATUS.lock().await.insert(url, status);
    }

    let has_sub_nodes = !final_node_names.is_empty();

    let sing_box_config = build_sing_box_config(
        config,
        my_names,
        my_outbounds,
        final_node_names,
        final_outbounds,
    )?;

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
    Ok(has_sub_nodes)
}

fn collect_manual_outbounds(config: &Config) -> (Vec<serde_json::Value>, Vec<String>) {
    let my_outbounds: Vec<serde_json::Value> = config
        .nodes
        .iter()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect();
    let my_names: Vec<String> = my_outbounds
        .iter()
        .filter_map(|o| o.get("tag").and_then(|v| v.as_str()).map(String::from))
        .collect();

    (my_outbounds, my_names)
}

fn build_sing_box_config(
    config: &Config,
    my_names: Vec<String>,
    my_outbounds: Vec<serde_json::Value>,
    final_node_names: Vec<String>,
    final_outbounds: Vec<serde_json::Value>,
) -> AppResult<serde_json::Value> {
    let total_nodes = my_outbounds.len() + final_outbounds.len();
    if total_nodes == 0 {
        return Err(AppError::message(
            "No nodes available: all subscriptions failed and no manual nodes configured",
        ));
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

    if let Some(rules) = sing_box_config["route"]["rules"].as_array_mut() {
        for rule_str in &config.custom_rules {
            if let Ok(rule_json) = serde_json::from_str::<serde_json::Value>(rule_str) {
                rules.push(rule_json);
            } else {
                eprintln!("Failed to parse custom rule: {}", rule_str);
            }
        }
    }

    Ok(sing_box_config)
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

#[cfg(test)]
mod tests {
    use super::{build_sing_box_config, collect_manual_outbounds};
    use crate::models::Config;
    use serde_json::json;

    #[test]
    fn collect_manual_outbounds_ignores_invalid_json_nodes() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"manual-a","server":"a.example.com","server_port":443,"password":"p","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"insecure":true}}"#.to_string(),
                "{invalid-json".to_string(),
            ],
            custom_rules: vec![],
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        assert_eq!(outbounds.len(), 1);
        assert_eq!(names, vec!["manual-a"]);
        assert_eq!(outbounds[0]["tag"], "manual-a");
    }

    #[test]
    fn build_sing_box_config_merges_nodes_and_valid_custom_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![
                r#"{"domain_suffix":["example.com"],"action":"route","outbound":"proxy"}"#.to_string(),
                "not-json".to_string(),
            ],
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];
        let final_outbounds = vec![json!({
            "type": "shadowsocks",
            "tag": "sub-a",
            "server": "sub.example.com",
            "server_port": 8388,
            "method": "2022-blake3-aes-128-gcm",
            "password": "sub-secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec!["sub-a".to_string()],
            final_outbounds,
        )
        .unwrap();

        let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
        assert_eq!(selector.len(), 2);
        assert_eq!(selector[0], "manual-a");
        assert_eq!(selector[1], "sub-a");

        let all_outbounds = built["outbounds"].as_array().unwrap();
        assert_eq!(all_outbounds.len(), 4);
        assert_eq!(all_outbounds[2]["tag"], "manual-a");
        assert_eq!(all_outbounds[3]["tag"], "sub-a");

        let rules = built["route"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[3]["domain_suffix"][0], "example.com");
    }

    #[test]
    fn build_sing_box_config_errors_when_no_nodes_available() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        };

        let err = build_sing_box_config(&config, vec![], vec![], vec![], vec![]).unwrap_err();

        assert!(err
            .to_string()
            .contains("No nodes available: all subscriptions failed and no manual nodes configured"));
    }
}
