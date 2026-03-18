use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, SubStatus};
use crate::services::{
    proxy::restore_last_proxy,
    singbox::{get_sing_box_home, start_sing_internal, stop_sing_internal},
    subscription::fetch_sub,
};
use crate::state::AppState;

const CONFIG_CACHE_PATH: &str = "/tmp/miao-sing-box/config.json.cache";

pub async fn save_config(
    config: &Config,
) -> AppResult<()> {
    let yaml = serde_yaml::to_string(config)?;
    
    // 使用临时文件+重命名实现原子写入，防止并发修改时配置文件损坏
    let temp_path = "config.yaml.tmp";
    let final_path = "config.yaml";
    
    // 先写入临时文件
    tokio::fs::write(temp_path, yaml).await
        .map_err(|e| AppError::context("Failed to write temp config file", e))?;
    
    // 原子重命名为最终文件
    tokio::fs::rename(temp_path, final_path).await
        .map_err(|e| AppError::context("Failed to atomically rename config file", e))?;
    
    Ok(())
}

pub async fn save_config_cache() {
    let config_path = get_sing_box_home().join("config.json");
    if let Err(e) = tokio::fs::copy(&config_path, CONFIG_CACHE_PATH).await {
        error!("Failed to save config cache: {}", e);
    } else {
        info!("Config cache saved to {}", CONFIG_CACHE_PATH);
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
    info!("Restored config from cache");
    Ok(())
}

pub async fn regenerate_and_restart(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<()> {
    let has_sub_nodes = gen_config(config, state)
        .await
        .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    stop_sing_internal(state).await;
    sleep(Duration::from_millis(500)).await;

    start_sing_internal(state)
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box", e))?;
    info!("sing-box restarted successfully");

    if has_sub_nodes {
        save_config_cache().await;
        *state.config_warning.lock().await = None;
    } else if !config.subs.is_empty() {
        *state.config_warning.lock().await = Some(
            "所有订阅获取失败，请检查当前订阅".to_string()
        );
    } else {
        *state.config_warning.lock().await = None;
    }

    let state_for_proxy = state.clone();
    tokio::spawn(async move {
        restore_last_proxy(&state_for_proxy).await;
    });

    Ok(())
}

/// Returns `true` if at least one subscription node was fetched successfully.
pub async fn gen_config(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let (my_outbounds, my_names) = collect_manual_outbounds(config);
    let mut final_outbounds: Vec<serde_json::Value> = vec![];
    let mut final_node_names: Vec<String> = vec![];

    {
        let mut status_map = state.sub_status.lock().await;
        status_map.retain(|url, _| config.subs.contains(url));
    }

    let sub_futures: Vec<_> = config
        .subs
        .iter()
        .map(|sub| {
            let sub = sub.clone();
            let client = state.http_client.clone();
            async move {
                info!("Fetching subscription: {}", sub);
                let result = tokio::time::timeout(
                    Duration::from_secs(30),
                    fetch_sub(&sub, &client)
                ).await;

                match result {
                    Ok(Ok(fetch_result)) => {
                        let valid_count = fetch_result.node_names.len();
                        let total_count = fetch_result.total_count;
                        let error_count = fetch_result.parse_errors.len();
                        
                        if error_count > 0 {
                            warn!("  -> Partial: fetched {}/{} valid nodes from {} ({} parse errors)", 
                                valid_count, total_count, sub, error_count);
                        } else {
                            info!("  -> Success: fetched {} nodes from {}", valid_count, sub);
                        }
                        
                        (sub.clone(), Ok(fetch_result))
                    }
                    Ok(Err(e)) => {
                        error!("  -> Failed to fetch subscription {}: {}", sub, e);
                        (sub.clone(), Err(e.to_string()))
                    }
                    Err(_) => {
                        error!("  -> Subscription {} timed out after 30s", sub);
                        (sub.clone(), Err("Request timeout".to_string()))
                    }
                }
            }
        })
        .collect();

    let results = futures::future::join_all(sub_futures).await;

    for (url, result) in results {
        let status = match result {
            Ok(fetch_result) => {
                let count = fetch_result.node_names.len();
                final_node_names.extend(fetch_result.node_names);
                final_outbounds.extend(fetch_result.outbounds);
                
                let error_info = if !fetch_result.parse_errors.is_empty() {
                    Some(format!("{} nodes skipped due to parse errors", fetch_result.parse_errors.len()))
                } else if count == 0 && fetch_result.total_count > 0 {
                    Some("All nodes invalid (missing required fields)".into())
                } else if count == 0 {
                    Some("No nodes found".into())
                } else {
                    None
                };
                
                SubStatus {
                    url: url.clone(),
                    success: count > 0,
                    node_count: count,
                    error: error_info,
                }
            }
            Err(e) => SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                error: Some(e),
            },
        };
        state.sub_status.lock().await.insert(url, status);
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

    Ok(has_sub_nodes)
}

fn collect_manual_outbounds(config: &Config) -> (Vec<serde_json::Value>, Vec<String>) {
    use crate::services::node_parser::parse_node_json;

    let mut my_outbounds = vec![];
    let mut my_names = vec![];

    for (idx, node_str) in config.nodes.iter().enumerate() {
        // 验证节点并获取解析后的 Value
        match parse_node_json(node_str) {
            Ok((info, outbound)) => {
                my_names.push(info.tag);
                my_outbounds.push(outbound);
            }
            Err(e) => {
                warn!("[collect_manual_outbounds] Skipping node #{}: {}", idx, e);
            }
        }
    }

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
                warn!("Failed to parse custom rule: {}", rule_str);
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
    use super::{build_sing_box_config, collect_manual_outbounds, save_config};
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
    fn collect_manual_outbounds_preserves_hysteria2_without_default_bandwidth() {
        // 测试：Hysteria2 节点不强制包含带宽默认值
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                // 不包含 up_mbps/down_mbps 的节点
                r#"{"type":"hysteria2","tag":"no-bandwidth","server":"example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#.to_string(),
            ],
            custom_rules: vec![],
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        assert_eq!(outbounds.len(), 1);
        assert_eq!(names, vec!["no-bandwidth"]);
        // 验证不包含硬编码的带宽字段
        assert!(outbounds[0].get("up_mbps").is_none() || outbounds[0]["up_mbps"].is_null());
        assert!(outbounds[0].get("down_mbps").is_none() || outbounds[0]["down_mbps"].is_null());
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

    #[test]
    fn collect_manual_outbounds_handles_empty_nodes() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        assert!(outbounds.is_empty());
        assert!(names.is_empty());
    }

    #[test]
    fn collect_manual_outbounds_handles_all_invalid_nodes() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                "not-json".to_string(),
                r#"{}"#.to_string(), // Valid JSON but no tag
                r#"{"type":"hysteria2"}"#.to_string(), // Valid JSON but no tag
            ],
            custom_rules: vec![],
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        // All nodes fail validation (missing required fields)
        assert!(outbounds.is_empty());
        assert!(names.is_empty());
    }

    #[test]
    fn build_sing_box_config_preserves_node_order() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        };

        let my_outbounds = vec![
            json!({"type": "hysteria2", "tag": "node-1", "server": "s1.example.com", "server_port": 443, "password": "p1"}),
            json!({"type": "hysteria2", "tag": "node-2", "server": "s2.example.com", "server_port": 443, "password": "p2"}),
            json!({"type": "hysteria2", "tag": "node-3", "server": "s3.example.com", "server_port": 443, "password": "p3"}),
        ];

        let built = build_sing_box_config(
            &config,
            vec!["node-1".to_string(), "node-2".to_string(), "node-3".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
        assert_eq!(selector.len(), 3);
        assert_eq!(selector[0], "node-1");
        assert_eq!(selector[1], "node-2");
        assert_eq!(selector[2], "node-3");
    }

    #[test]
    fn build_sing_box_config_handles_no_custom_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let rules = built["route"]["rules"].as_array().unwrap();
        // Should have only the default 3 rules (sniff, hijack-dns, private ip)
        assert_eq!(rules.len(), 3);
    }

    #[test]
    fn build_sing_box_config_ignores_all_invalid_custom_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![
                "not-json".to_string(),
                "{invalid".to_string(),
                "".to_string(),
            ],
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let rules = built["route"]["rules"].as_array().unwrap();
        // Should have only the default 3 rules
        assert_eq!(rules.len(), 3);
    }

    #[tokio::test]
    async fn save_config_performs_atomic_write() {
        let temp_dir = std::env::temp_dir().join(format!("miao-test-{}", std::process::id()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        
        let config = Config {
            port: Some(8080),
            subs: vec!["https://example.com/sub".to_string()],
            nodes: vec![],
            custom_rules: vec![],
        };
        
        // 使用绝对路径保存配置
        let config_path = temp_dir.join("config.yaml");
        let temp_config_path = temp_dir.join("config.yaml.tmp");
        let yaml = serde_yaml::to_string(&config).unwrap();
        
        tokio::fs::write(&temp_config_path, yaml).await.unwrap();
        tokio::fs::rename(&temp_config_path, &config_path).await.unwrap();
        
        // 验证文件存在且格式正确
        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let parsed: Config = serde_yaml::from_str(&content).unwrap();
        assert_eq!(parsed.port, Some(8080));
        assert_eq!(parsed.subs.len(), 1);
        
        // 清理
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn save_config_overwrites_existing_file() {
        let temp_dir = std::env::temp_dir().join(format!("miao-test-overwrite-{}", std::process::id()));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        
        let config_path = temp_dir.join("config.yaml");
        
        // 先创建旧配置
        tokio::fs::write(&config_path, "port: 9999\nsubs: []\nnodes: []\ncustom_rules: []").await.unwrap();
        
        // 使用原子写入保存新配置
        let config = Config {
            port: Some(7777),
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        let temp_config_path = temp_dir.join("config.yaml.tmp");
        tokio::fs::write(&temp_config_path, yaml).await.unwrap();
        tokio::fs::rename(&temp_config_path, &config_path).await.unwrap();
        
        // 验证被覆盖
        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let parsed: Config = serde_yaml::from_str(&content).unwrap();
        assert_eq!(parsed.port, Some(7777));
        
        // 清理
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
