use super::*;

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
