use super::apply::{
    apply_config_change, config_apply_mode, config_changed_after_refresh, no_usable_nodes_warning,
    persist_config_without_usable_nodes_at, sub_source_for, ConfigApplyMode, SubSource,
};
use super::builder::{build_sing_box_config, filter_rules_with_missing_outbound, tun_inbound};
use super::generate::{collect_manual_outbounds, runtime_config_node_tags};
use super::persist::save_config_to;
use crate::{
    models::{Config, NodeSelect, Region, RouteMode, SubStatus},
    test_support::app_state,
};
use serde_json::json;
use std::collections::HashSet;

use crate::state::SkippedRule;

#[test]
fn refresh_activate_decision_compares_bytes() {
    // 内容一致不激活，避免无意义断流
    assert!(!config_changed_after_refresh(Some(b"same"), Some(b"same")));
    assert!(config_changed_after_refresh(Some(b"old"), Some(b"new")));
    // 读不出内容时保守激活
    assert!(config_changed_after_refresh(None, Some(b"new")));
    assert!(config_changed_after_refresh(Some(b"old"), None));
}

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
            route_mode: Default::default(),
                mcp: false,
            node_select: Default::default(),
            disabled_nodes: Default::default(),
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
            route_mode: Default::default(),
                mcp: false,
            node_select: Default::default(),
            disabled_nodes: Default::default(),
        };

    let (outbounds, names) = collect_manual_outbounds(&config);

    assert_eq!(outbounds.len(), 1);
    assert_eq!(names, vec!["no-bandwidth"]);
    // 验证不包含硬编码的带宽字段
    assert!(outbounds[0].get("up_mbps").is_none() || outbounds[0]["up_mbps"].is_null());
    assert!(outbounds[0].get("down_mbps").is_none() || outbounds[0]["down_mbps"].is_null());
}

#[test]
fn filter_rules_skips_only_rules_with_missing_outbound() {
    let available: HashSet<String> = ["proxy", "direct", "node-a"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let rules = vec![
        r#"{"process_name":"curl","action":"route","outbound":"gone-node"}"#.to_string(),
        r#"{"process_name":"wget","action":"route","outbound":"node-a"}"#.to_string(),
        r#"{"domain":"t.co","action":"route","outbound":"proxy"}"#.to_string(),
        r#"{"domain_keyword":"ad","action":"reject"}"#.to_string(),
        "not-json".to_string(),
    ];

    let (kept, skipped) = filter_rules_with_missing_outbound(&rules, &available);

    // 失效规则与无法解析的规则都会在生成阶段跳过；原始配置仍保持不变
    assert_eq!(kept.len(), 3);
    assert!(!kept.iter().any(|r| r.contains("gone-node")));
    assert_eq!(
        skipped,
        vec![SkippedRule {
            raw: rules[0].clone(),
            description: "process_name=curl → gone-node".to_string(),
        }]
    );
}

#[test]
fn build_sing_box_config_skips_rules_with_missing_node() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![
            r#"{"process_path":"/opt/app/run","action":"route","outbound":"已消失节点"}"#
                .to_string(),
            r#"{"process_name":"curl","action":"route","outbound":"manual-a"}"#.to_string(),
        ],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let (built, skipped, _) = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            vec![json!({"type":"hysteria2","tag":"manual-a","server":"a.example.com","server_port":443,"password":"p","tls":{"enabled":true}})],
            vec![],
            vec![],
        )
        .unwrap();

    let rules = built["route"]["rules"].as_array().unwrap();
    let rules_json = serde_json::to_string(rules).unwrap();
    assert!(!rules_json.contains("已消失节点"));
    assert!(rules_json.contains("manual-a"));
    let dns_json = serde_json::to_string(&built["dns"]).unwrap();
    assert!(!dns_json.contains("已消失节点"));
    assert!(!built["dns"]["servers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|server| server["detour"] == "已消失节点"));
    assert_eq!(skipped.len(), 1);
    assert_eq!(
        skipped[0].description,
        "process_path=/opt/app/run → 已消失节点"
    );
    assert_eq!(skipped[0].raw, config.custom_rules[0]);
}

#[test]
fn generated_config_enables_find_process_for_panel_process_view() {
    // 面板「按进程」视图依赖 Clash API 的 processPath，而 sing-box 只在
    // find_process 或进程类规则存在时才收集——无条件开启保证任何用户配置下都有数据。
    let config = Config {
        nodes: vec![r#"{"type":"hysteria2","tag":"manual-a","server":"a.example.com","server_port":443,"password":"p","tls":{"enabled":true}}"#.to_string()],
        ..Default::default()
    };

    let (built, _, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        vec![json!({"type":"hysteria2","tag":"manual-a","server":"a.example.com","server_port":443,"password":"p","tls":{"enabled":true}})],
        vec![],
        vec![],
    )
    .unwrap();

    assert_eq!(built["route"]["find_process"], json!(true));
}

#[test]
fn runtime_config_node_tags_excludes_builtin_outbounds() {
    let config_json = json!({
        "outbounds": [
            {"type": "selector", "tag": "proxy"},
            {"type": "direct", "tag": "direct"},
            {"type": "hysteria2", "tag": "香港节点"},
            {"type": "shadowsocks", "tag": "ss-us"},
            {"type": "urltest"}
        ]
    });

    assert_eq!(
        runtime_config_node_tags(&config_json),
        vec!["香港节点", "ss-us"]
    );
    assert!(runtime_config_node_tags(&json!({})).is_empty());
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
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
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

    let (built, _skipped, _) = build_sing_box_config(
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
    assert_eq!(rules.len(), 6);
    assert_eq!(rules[0]["action"], "sniff");
    assert_eq!(rules[1]["action"], "hijack-dns");
    assert_eq!(rules[2]["domain_suffix"][0], "example.com");
    assert_eq!(rules[3]["ip_is_private"], true);

    let dns_rules = built["dns"]["rules"].as_array().unwrap();
    assert_eq!(dns_rules.len(), 2);
    assert_eq!(dns_rules[0]["domain_suffix"], json!(["example.com"]));
    assert_eq!(dns_rules[0]["server"], "cfdns");
    assert_eq!(dns_rules[1]["rule_set"], json!(["chinasite"]));
}

#[test]
fn build_sing_box_config_global_mode_removes_split_rules() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![
            r#"{"domain_suffix":["example.com"],"action":"route","outbound":"direct"}"#.to_string(),
        ],
        route_mode: RouteMode::Global,
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let my_outbounds = vec![json!({
        "type": "hysteria2",
        "tag": "manual-a",
        "server": "manual.example.com",
        "server_port": 443,
        "password": "secret"
    })];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        my_outbounds,
        vec![],
        vec![],
    )
    .unwrap();

    let rules = built["route"]["rules"].as_array().unwrap();
    // 内置分流规则被裁掉,但自定义规则在全局模式下仍然生效
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0]["action"], "sniff");
    assert_eq!(rules[1]["action"], "hijack-dns");
    assert_eq!(rules[2]["domain_suffix"], json!(["example.com"]));
    assert_eq!(rules[2]["outbound"], "direct");
    assert!(rules[..2].iter().all(|rule| rule.get("outbound").is_none()));

    let dns_rules = built["dns"]["rules"].as_array().unwrap();
    // 全局模式只移除内置中国分流；自定义直连规则的 DNS 策略仍保留。
    assert_eq!(dns_rules.len(), 1);
    assert_eq!(dns_rules[0]["domain_suffix"], json!(["example.com"]));
    assert_eq!(dns_rules[0]["server"], "local");
    assert_eq!(built["route"]["final"], "proxy");
}

#[test]
fn build_sing_box_config_mirrors_safe_custom_rules_to_dns() {
    let config = Config {
        custom_rules: vec![
            r#"{"domain_keyword":"openai","action":"route","outbound":"manual-a"}"#.to_string(),
            r#"{"domain_suffix":["example.com"],"action":"route","outbound":"manual-a"}"#
                .to_string(),
            r#"{"process_name":"curl","action":"route","outbound":"direct"}"#.to_string(),
            // The destination port describes the data connection, not its DNS query.
            r#"{"port":443,"action":"route","outbound":"manual-a"}"#.to_string(),
            // Do not broaden a compound raw rule by dropping its unsupported matcher.
            r#"{"domain":"mixed.example","port":443,"action":"route","outbound":"manual-a"}"#
                .to_string(),
        ],
        ..Default::default()
    };
    let my_outbounds = vec![json!({
        "type": "hysteria2",
        "tag": "manual-a",
        "server": "manual.example.com",
        "server_port": 443,
        "password": "secret"
    })];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        my_outbounds,
        vec![],
        vec![],
    )
    .unwrap();

    let dns_servers = built["dns"]["servers"].as_array().unwrap();
    let node_servers: Vec<_> = dns_servers
        .iter()
        .filter(|server| server["tag"] == "custom-node-dns-1")
        .collect();
    assert_eq!(node_servers.len(), 1);
    assert_eq!(node_servers[0]["type"], "https");
    assert_eq!(node_servers[0]["server"], "1.1.1.1");
    assert_eq!(node_servers[0]["detour"], "manual-a");

    let dns_rules = built["dns"]["rules"].as_array().unwrap();
    assert_eq!(dns_rules.len(), 4);
    assert_eq!(dns_rules[0]["domain_keyword"], "openai");
    assert_eq!(dns_rules[0]["server"], "custom-node-dns-1");
    assert_eq!(dns_rules[1]["domain_suffix"], json!(["example.com"]));
    assert_eq!(dns_rules[1]["server"], "custom-node-dns-1");
    assert_eq!(dns_rules[2]["process_name"], "curl");
    assert_eq!(dns_rules[2]["server"], "local");
    assert_eq!(dns_rules[3]["rule_set"], json!(["chinasite"]));
    assert!(dns_rules.iter().all(|rule| rule.get("port").is_none()));
}

#[test]
fn config_change_clears_runtime_when_last_source_is_removed() {
    let config = Config::default();

    assert_eq!(config_apply_mode(&config, true), ConfigApplyMode::Clear);
    assert_eq!(config_apply_mode(&config, false), ConfigApplyMode::Clear);
}

#[tokio::test]
async fn clearing_the_last_source_drops_node_bindings() {
    let old = Config {
        nodes: vec![
            r#"{"type":"hysteria2","tag":"ghost","server":"127.0.0.1","server_port":443,"password":"x"}"#.to_string(),
        ],
        ..Config::default()
    };
    let state = app_state(old.clone());
    if let Some(parent) = state.runtime_paths.node_bindings.parent() {
        tokio::fs::create_dir_all(parent).await.unwrap();
    }
    tokio::fs::write(
        &state.runtime_paths.node_bindings,
        b"{\"version\":1,\"bindings\":[]}",
    )
    .await
    .unwrap();

    apply_config_change(&state, &old, &Config::default())
        .await
        .unwrap();

    assert!(!state.runtime_paths.node_bindings.exists());
}

#[test]
fn sub_source_is_snapshot_when_subs_unchanged() {
    let old = Config {
        subs: vec!["https://a.example.com".to_string()],
        ..Config::default()
    };
    // 节点选择/规则/MCP/手动节点等本地语义变更不动 subs → 快照重建
    let mut new = old.clone();
    new.mcp = true;
    assert_eq!(sub_source_for(&old, &new), SubSource::SnapshotOrFetch);

    let mut new = old.clone();
    new.nodes.push("manual-node".to_string());
    assert_eq!(sub_source_for(&old, &new), SubSource::SnapshotOrFetch);

    // 增删订阅 → 必须真拉取
    let mut new = old.clone();
    new.subs.push("https://b.example.com".to_string());
    assert_eq!(sub_source_for(&old, &new), SubSource::Fetch);

    let new = Config {
        subs: vec![],
        nodes: vec!["manual-node".to_string()],
        ..Config::default()
    };
    assert_eq!(sub_source_for(&old, &new), SubSource::Fetch);
}

#[test]
fn unusable_node_warning_distinguishes_manual_and_subscription_configs() {
    let manual = Config {
        nodes: vec!["invalid-node".to_string()],
        ..Config::default()
    };
    let subscription = Config {
        subs: vec!["https://example.com/sub".to_string()],
        ..Config::default()
    };

    assert!(no_usable_nodes_warning(&manual).contains("手动节点"));
    assert!(no_usable_nodes_warning(&subscription).contains("订阅"));
}

#[tokio::test]
async fn unusable_config_is_persisted_and_stale_runtime_files_are_removed() {
    let state = app_state(Config::default());
    let temp_dir =
        std::env::temp_dir().join(format!("miao-unusable-config-{}", std::process::id()));
    let runtime_path = temp_dir.join("config.json");
    let cache_path = temp_dir.join("config.json.cache");
    let sub_nodes_path = temp_dir.join("sub-nodes.json");
    let bindings_path = temp_dir.join("node-bindings.json");
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();
    tokio::fs::write(&runtime_path, "stale").await.unwrap();
    tokio::fs::write(&cache_path, "stale").await.unwrap();
    tokio::fs::write(&sub_nodes_path, "stale").await.unwrap();
    tokio::fs::write(&bindings_path, b"{\"version\":1,\"bindings\":[]}")
        .await
        .unwrap();

    let subscription_url = "https://example.com/broken".to_string();
    state.sub_status.lock().await.insert(
        subscription_url.clone(),
        SubStatus {
            url: subscription_url.clone(),
            success: false,
            node_count: 0,
            disabled_count: 0,
            state: crate::models::SubscriptionState::Failed,
            error: Some("fetch failed".to_string()),
        },
    );
    let config = Config {
        subs: vec![subscription_url.clone()],
        ..Config::default()
    };

    persist_config_without_usable_nodes_at(
        &state,
        config,
        &runtime_path,
        &cache_path,
        &sub_nodes_path,
    )
    .await
    .unwrap();

    assert!(!runtime_path.exists());
    assert!(!cache_path.exists());
    assert!(!sub_nodes_path.exists());
    assert!(
        bindings_path.exists(),
        "subscription URLs remain; tag bindings must survive a tmpfs wipe"
    );
    assert_eq!(
        state.config.read().await.subs,
        vec![subscription_url.clone()]
    );
    assert!(state.config_warning.lock().await.is_some());
    assert!(state
        .sub_status
        .lock()
        .await
        .contains_key(&subscription_url));
    let persisted = tokio::fs::read_to_string(&state.config_path).await.unwrap();
    assert!(persisted.contains(&subscription_url));

    let _ = tokio::fs::remove_file(&state.config_path).await;
    let _ = tokio::fs::remove_dir_all(temp_dir).await;
}

#[test]
fn config_change_preserves_explicitly_stopped_service() {
    let config = Config {
        nodes: vec![r#"{"type":"hysteria2"}"#.to_string()],
        ..Config::default()
    };

    assert_eq!(
        config_apply_mode(&config, false),
        ConfigApplyMode::RegenerateOnly
    );
}

#[test]
fn config_change_activates_service_when_it_is_desired() {
    let config = Config {
        subs: vec!["https://example.com/sub".to_string()],
        ..Config::default()
    };

    assert_eq!(config_apply_mode(&config, true), ConfigApplyMode::Restart);
}

#[test]
fn build_sing_box_config_renames_duplicate_outbound_tags() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let my_outbounds = vec![json!({
        "type": "hysteria2",
        "tag": "dup",
        "server": "manual.example.com",
        "server_port": 443,
        "password": "manual-secret"
    })];
    let final_outbounds = vec![
        json!({
            "type": "hysteria2",
            "tag": "dup",
            "server": "sub1.example.com",
            "server_port": 443,
            "password": "sub-secret-1"
        }),
        json!({
            "type": "shadowsocks",
            "tag": "dup",
            "server": "sub2.example.com",
            "server_port": 8388,
            "method": "2022-blake3-aes-128-gcm",
            "password": "sub-secret-2"
        }),
    ];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["dup".to_string()],
        my_outbounds,
        vec!["dup".to_string(), "dup".to_string()],
        final_outbounds,
    )
    .unwrap();

    let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
    let selector_tags: Vec<_> = selector
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(selector_tags, vec!["dup", "dup (2)", "dup (3)"]);

    let all_outbounds = built["outbounds"].as_array().unwrap();
    assert_eq!(all_outbounds[2]["tag"], "dup");
    assert_eq!(all_outbounds[3]["tag"], "dup (2)");
    assert_eq!(all_outbounds[4]["tag"], "dup (3)");
}

#[test]
fn build_sing_box_config_renames_tags_reserved_by_template() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let my_outbounds = vec![
        json!({
            "type": "hysteria2",
            "tag": "proxy",
            "server": "proxy.example.com",
            "server_port": 443,
            "password": "proxy-secret"
        }),
        json!({
            "type": "hysteria2",
            "tag": "direct",
            "server": "direct.example.com",
            "server_port": 443,
            "password": "direct-secret"
        }),
    ];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["proxy".to_string(), "direct".to_string()],
        my_outbounds,
        vec![],
        vec![],
    )
    .unwrap();

    let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
    let selector_tags: Vec<_> = selector
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(selector_tags, vec!["proxy (2)", "direct (2)"]);

    let all_outbounds = built["outbounds"].as_array().unwrap();
    assert_eq!(all_outbounds[0]["tag"], "proxy");
    assert_eq!(all_outbounds[1]["tag"], "direct");
    assert_eq!(all_outbounds[2]["tag"], "proxy (2)");
    assert_eq!(all_outbounds[3]["tag"], "direct (2)");
}

#[test]
fn build_sing_box_config_errors_when_no_nodes_available() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let err = build_sing_box_config(&config, vec![], vec![], vec![], vec![]).unwrap_err();

    assert!(err.is_no_usable_nodes());
    assert!(err
        .to_string()
        .contains("No usable nodes available: subscriptions failed or manual nodes were invalid"));
}

#[test]
fn collect_manual_outbounds_handles_empty_nodes() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
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
            r#"{}"#.to_string(),                   // Valid JSON but no tag
            r#"{"type":"hysteria2"}"#.to_string(), // Valid JSON but no tag
        ],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
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
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let my_outbounds = vec![
        json!({"type": "hysteria2", "tag": "node-1", "server": "s1.example.com", "server_port": 443, "password": "p1"}),
        json!({"type": "hysteria2", "tag": "node-2", "server": "s2.example.com", "server_port": 443, "password": "p2"}),
        json!({"type": "hysteria2", "tag": "node-3", "server": "s3.example.com", "server_port": 443, "password": "p3"}),
    ];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec![
            "node-1".to_string(),
            "node-2".to_string(),
            "node-3".to_string(),
        ],
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
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let my_outbounds = vec![json!({
        "type": "hysteria2",
        "tag": "manual-a",
        "server": "manual.example.com",
        "server_port": 443,
        "password": "secret"
    })];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        my_outbounds,
        vec![],
        vec![],
    )
    .unwrap();

    let rules = built["route"]["rules"].as_array().unwrap();
    // Should have the default direct-split rules.
    assert_eq!(rules.len(), 5);
}

#[test]
fn build_sing_box_config_splits_direct_route_rules() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let my_outbounds = vec![json!({
        "type": "hysteria2",
        "tag": "manual-a",
        "server": "manual.example.com",
        "server_port": 443,
        "password": "secret"
    })];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        my_outbounds,
        vec![],
        vec![],
    )
    .unwrap();

    let rules = built["route"]["rules"].as_array().unwrap();

    assert_eq!(rules[2]["ip_is_private"], true);
    assert_eq!(rules[2]["outbound"], "direct");
    assert!(rules[2].get("rule_set").is_none());

    assert_eq!(rules[3]["rule_set"], json!(["chinasite"]));
    assert_eq!(rules[3]["outbound"], "direct");
    assert!(rules[3].get("ip_is_private").is_none());

    assert_eq!(rules[4]["rule_set"], json!(["chinaip"]));
    assert_eq!(rules[4]["outbound"], "direct");
    assert!(rules[4].get("ip_is_private").is_none());

    let dns_rules = built["dns"]["rules"].as_array().unwrap();
    assert_eq!(dns_rules.len(), 1);
    assert_eq!(dns_rules[0]["rule_set"], json!(["chinasite"]));
    assert_eq!(dns_rules[0]["server"], "local");

    assert_eq!(built["dns"]["disable_cache"], false);
    assert_eq!(built["dns"]["reverse_mapping"], true);
    assert_eq!(built["dns"]["cache_capacity"], 4096);
    assert_eq!(built["dns"]["optimistic"]["enabled"], true);
    assert_eq!(built["dns"]["optimistic"]["timeout"], "8h");

    let dns_servers = built["dns"]["servers"].as_array().unwrap();
    let cfdns = dns_servers
        .iter()
        .find(|server| server["tag"] == "cfdns")
        .unwrap();
    assert_eq!(cfdns["type"], "https");
    assert_eq!(cfdns["server"], "1.1.1.1");
    assert_eq!(cfdns["detour"], "proxy");

    let local = dns_servers
        .iter()
        .find(|server| server["tag"] == "local")
        .unwrap();
    assert_eq!(local["type"], "udp");
    assert_eq!(local["server"], "223.5.5.5");
    assert!(local.get("detour").is_none());

    assert!(dns_servers
        .iter()
        .all(|server| server["type"] != "fakeip" && server["tag"] != "fakeip"));

    assert_eq!(built["experimental"]["cache_file"]["enabled"], true);
    assert_eq!(built["experimental"]["cache_file"]["path"], "cache.db");
    assert_eq!(built["experimental"]["cache_file"]["store_dns"], true);
}

#[test]
fn build_sing_box_config_binds_clash_api_to_localhost() {
    let config = Config {
        port: None,
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })],
        vec![],
        vec![],
    )
    .unwrap();

    assert_eq!(
        built["experimental"]["clash_api"]["external_controller"],
        crate::services::singbox::CLASH_API_HOST
    );
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
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    let my_outbounds = vec![json!({
        "type": "hysteria2",
        "tag": "manual-a",
        "server": "manual.example.com",
        "server_port": 443,
        "password": "secret"
    })];

    let (built, _skipped, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        my_outbounds,
        vec![],
        vec![],
    )
    .unwrap();

    let rules = built["route"]["rules"].as_array().unwrap();
    // Should have only the default direct-split rules.
    assert_eq!(rules.len(), 5);
}

#[tokio::test]
async fn save_config_performs_atomic_write() {
    let temp_dir = std::env::temp_dir().join(format!(
        "miao-test-save-{}-{}",
        std::process::id(),
        "atomic"
    ));
    let config_path = temp_dir.join("nested").join("config.yaml");

    let config = Config {
        port: Some(8080),
        subs: vec!["https://example.com/sub".to_string()],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    save_config_to(&config_path, &config).await.unwrap();

    let content = tokio::fs::read_to_string(&config_path).await.unwrap();
    let parsed: Config = yaml_serde::from_str(&content).unwrap();
    assert_eq!(parsed.port, Some(8080));
    assert_eq!(parsed.subs.len(), 1);

    // 清理
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[tokio::test]
async fn save_config_overwrites_existing_file() {
    let temp_dir = std::env::temp_dir().join(format!(
        "miao-test-save-{}-{}",
        std::process::id(),
        "overwrite"
    ));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let config_path = temp_dir.join("config.yaml");

    // 先创建旧配置
    tokio::fs::write(
        &config_path,
        "port: 9999\nsubs: []\nnodes: []\ncustom_rules: []",
    )
    .await
    .unwrap();

    // 使用原子写入保存新配置
    let config = Config {
        port: Some(7777),
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };
    save_config_to(&config_path, &config).await.unwrap();

    let content = tokio::fs::read_to_string(&config_path).await.unwrap();
    let parsed: Config = yaml_serde::from_str(&content).unwrap();
    assert_eq!(parsed.port, Some(7777));

    // 清理
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[tokio::test]
async fn save_config_skips_identical_content() {
    let temp_dir =
        std::env::temp_dir().join(format!("miao-test-save-{}-{}", std::process::id(), "skip"));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let config_path = temp_dir.join("config.yaml");
    let config = Config {
        port: Some(6161),
        subs: vec![],
        nodes: vec![],
        custom_rules: vec![],
        route_mode: Default::default(),
        mcp: false,
        node_select: Default::default(),
        disabled_nodes: Default::default(),
    };

    save_config_to(&config_path, &config).await.unwrap();
    let before = tokio::fs::metadata(&config_path)
        .await
        .unwrap()
        .modified()
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    save_config_to(&config_path, &config).await.unwrap();

    let after = tokio::fs::metadata(&config_path)
        .await
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[test]
fn tun_inbound_enables_auto_redirect_only_on_linux() {
    let inbound = tun_inbound();
    assert_eq!(inbound["type"], "tun");
    assert_eq!(inbound["auto_route"], true);
    assert_eq!(inbound["strict_route"], true);
    if cfg!(target_os = "linux") {
        assert_eq!(inbound["auto_redirect"], true);
    } else {
        assert!(inbound.get("auto_redirect").is_none());
    }
}

fn sample_nodes() -> (
    Vec<String>,
    Vec<serde_json::Value>,
    Vec<String>,
    Vec<serde_json::Value>,
) {
    (
        vec!["香港-手动".to_string()],
        vec![json!({
            "type": "hysteria2",
            "tag": "香港-手动",
            "server": "hk.example.com",
            "server_port": 443,
            "password": "secret"
        })],
        vec!["日本-订阅".to_string(), "新加坡-订阅".to_string()],
        vec![
            json!({
                "type": "shadowsocks",
                "tag": "日本-订阅",
                "server": "jp.example.com",
                "server_port": 8388,
                "method": "aes-128-gcm",
                "password": "secret"
            }),
            json!({
                "type": "shadowsocks",
                "tag": "新加坡-订阅",
                "server": "sg.example.com",
                "server_port": 8388,
                "method": "aes-128-gcm",
                "password": "secret"
            }),
        ],
    )
}

#[test]
fn build_sing_box_config_uses_urltest_for_region_fastest() {
    let config = Config {
        node_select: NodeSelect::Fastest(Region::Jp),
        ..Config::default()
    };
    let (my_names, my_outbounds, sub_names, sub_outbounds) = sample_nodes();
    let (built, _skipped, effective) =
        build_sing_box_config(&config, my_names, my_outbounds, sub_names, sub_outbounds).unwrap();

    assert_eq!(effective, NodeSelect::Fastest(Region::Jp));
    assert_eq!(built["outbounds"][0]["type"], "urltest");
    assert_eq!(built["outbounds"][0]["tag"], "proxy");
    assert_eq!(built["outbounds"][0]["outbounds"], json!(["日本-订阅"]));
    assert_eq!(built["outbounds"][0]["interval"], "2m");
    assert_eq!(built["outbounds"][0]["tolerance"], 30);
    assert_eq!(built["outbounds"][0]["interrupt_exist_connections"], false);
    let tags: Vec<&str> = built["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item.get("tag").and_then(|tag| tag.as_str()))
        .collect();
    assert!(tags.contains(&"香港-手动"));
    assert!(tags.contains(&"新加坡-订阅"));
}

#[test]
fn build_sing_box_config_falls_back_to_selector_when_region_empty() {
    let config = Config {
        node_select: NodeSelect::Fastest(Region::Us),
        ..Config::default()
    };
    let (my_names, my_outbounds, sub_names, sub_outbounds) = sample_nodes();
    let (built, _skipped, effective) =
        build_sing_box_config(&config, my_names, my_outbounds, sub_names, sub_outbounds).unwrap();

    assert_eq!(effective, NodeSelect::Manual);
    assert_eq!(built["outbounds"][0]["type"], "selector");
    assert_eq!(
        built["outbounds"][0]["outbounds"].as_array().unwrap().len(),
        3
    );
}
