use super::*;

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
            max_multiplier: None,
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
            max_multiplier: None,
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
fn missing_outbound_rules_block_in_place_and_recover_without_changing_raw_rules() {
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

    let (kept, skipped) = block_rules_with_missing_outbound(&rules, &available);

    assert_eq!(kept.len(), 4);
    assert_eq!(kept[0], json!({"process_name":"curl","action":"reject"}));
    for index in 1..4 {
        assert_eq!(
            kept[index],
            serde_json::from_str::<serde_json::Value>(&rules[index]).unwrap()
        );
    }
    assert_eq!(
        skipped,
        vec![SkippedRule {
            raw: rules[0].clone(),
            description: "process_name=curl → gone-node".to_string(),
        }]
    );
    let mut restored = available;
    restored.insert("gone-node".to_string());
    let (recovered, warnings) = block_rules_with_missing_outbound(&rules, &restored);
    assert!(warnings.is_empty());
    assert_eq!(
        recovered[0],
        serde_json::from_str::<serde_json::Value>(&rules[0]).unwrap()
    );
}

#[test]
fn missing_outbound_blocking_preserves_compound_and_logical_matchers() {
    let matchers = [
        json!({"domain_suffix":["example.com"],"network":"tcp","port":443,"invert":true}),
        json!({"type":"logical","mode":"and","rules":[{"domain":"example.com"},{"process_name":"curl"}]}),
        json!({"ip_cidr":"192.0.2.0/24","source_ip_cidr":"10.0.0.0/8"}),
    ];
    for matcher in matchers {
        let mut original = matcher.clone();
        original["outbound"] = json!("gone-node");
        original["override_address"] = json!("127.0.0.1");
        original["override_port"] = json!(8443);
        original["udp_timeout"] = json!("1m");
        original["tls_fragment"] = json!(true);
        let raw = vec![original.to_string()];
        let (blocked, warnings) = block_rules_with_missing_outbound(&raw, &HashSet::new());
        let mut expected = matcher;
        expected["action"] = json!("reject");
        assert_eq!(blocked, vec![expected]);
        assert_eq!(warnings[0].raw, raw[0]);
        let (restored, warnings) =
            block_rules_with_missing_outbound(&raw, &HashSet::from(["gone-node".to_string()]));
        assert_eq!(restored, vec![original]);
        assert!(warnings.is_empty());
    }
}

#[test]
fn build_sing_box_config_blocks_rules_with_missing_node() {
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
        max_multiplier: None,
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
    assert_eq!(
        rules[2],
        json!({"process_path":"/opt/app/run","action":"reject"})
    );
    assert_eq!(
        built["dns"]["rules"][0],
        json!({"process_path":"/opt/app/run","action":"reject"})
    );
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
fn blocked_dns_rules_do_not_broaden_connection_only_matchers() {
    let config = Config {
        custom_rules: vec![
            r#"{"domain":"example.com","port":443,"outbound":"gone"}"#.to_string(),
            r#"{"process_name":"curl","network":"tcp","outbound":"gone"}"#.to_string(),
            r#"{"ip_cidr":"192.0.2.0/24","outbound":"gone"}"#.to_string(),
        ],
        ..Config::default()
    };
    let (built, blocked, _) = build_sing_box_config(
        &config,
        vec!["manual-a".to_string()],
        vec![json!({"type":"direct","tag":"manual-a"})],
        vec![],
        vec![],
    )
    .unwrap();
    assert_eq!(blocked.len(), 3);
    for (index, raw) in config.custom_rules.iter().enumerate() {
        let mut expected: serde_json::Value = serde_json::from_str(raw).unwrap();
        expected.as_object_mut().unwrap().remove("outbound");
        expected["action"] = json!("reject");
        assert_eq!(built["route"]["rules"][index + 2], expected);
    }
    assert_eq!(
        built["dns"]["rules"],
        json!([{"rule_set":["chinasite"],"action":"route","server":"local"}])
    );
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
        max_multiplier: None,
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
        max_multiplier: None,
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
        .filter(|server| server["tag"] == "custom-node-dns:manual-a")
        .collect();
    assert_eq!(node_servers.len(), 1);
    assert_eq!(node_servers[0]["type"], "https");
    assert_eq!(node_servers[0]["server"], "1.1.1.1");
    assert_eq!(node_servers[0]["detour"], "manual-a");

    let dns_rules = built["dns"]["rules"].as_array().unwrap();
    assert_eq!(dns_rules.len(), 4);
    assert_eq!(dns_rules[0]["domain_keyword"], "openai");
    assert_eq!(dns_rules[0]["server"], "custom-node-dns:manual-a");
    assert_eq!(dns_rules[1]["domain_suffix"], json!(["example.com"]));
    assert_eq!(dns_rules[1]["server"], "custom-node-dns:manual-a");
    assert_eq!(dns_rules[2]["process_name"], "curl");
    assert_eq!(dns_rules[2]["server"], "local");
    assert_eq!(dns_rules[3]["rule_set"], json!(["chinasite"]));
    assert!(dns_rules.iter().all(|rule| rule.get("port").is_none()));
}

#[test]
fn dns_cache_identity_survives_rule_reordering_and_other_nodes_disappearing() {
    let a = r#"{"source_ip_cidr":"192.168.50.101/32","outbound":"employee-a"}"#;
    let b = r#"{"source_ip_cidr":"192.168.50.102/32","outbound":"employee-b"}"#;
    let mut b_server = None;
    for (rules, nodes) in [
        (vec![a, b], vec!["employee-a", "employee-b"]),
        (vec![b, a], vec!["employee-a", "employee-b"]),
        (vec![a, b], vec!["employee-b"]),
    ] {
        let config = Config {
            custom_rules: rules.into_iter().map(str::to_string).collect(),
            ..Default::default()
        };
        let outbounds = nodes
            .iter()
            .map(|tag| json!({"type":"direct", "tag":tag}))
            .collect();
        let (built, _, _) = build_sing_box_config(
            &config,
            nodes.into_iter().map(str::to_string).collect(),
            outbounds,
            vec![],
            vec![],
        )
        .unwrap();
        let server = built["dns"]["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server["detour"] == "employee-b")
            .unwrap();
        assert_eq!(b_server.get_or_insert_with(|| server.clone()), server);
    }
}
