use super::*;
use crate::services::config::apply_node_select;

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
    // 与 Clash API 测速端点同口径（sing-box API 层拒绝 http:// 测速 URL）
    assert_eq!(
        built["outbounds"][0]["url"],
        "https://www.gstatic.com/generate_204"
    );
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

#[tokio::test]
async fn explicit_node_selection_is_persisted_even_when_runtime_is_unchanged() {
    let state = app_state(Config::default());

    let (previous, effective, update) =
        apply_node_select(&state, NodeSelect::Manual).await.unwrap();

    assert_eq!(previous, NodeSelect::Manual);
    assert_eq!(effective, NodeSelect::Manual);
    assert_eq!(update, RuntimeUpdate::None);
    assert_eq!(
        tokio::fs::read_to_string(&state.runtime_paths.node_select_preference)
            .await
            .unwrap(),
        "manual\n"
    );
    let _ = tokio::fs::remove_dir_all(&state.runtime_paths.runtime_dir).await;
}

#[tokio::test]
async fn concurrent_node_selection_keeps_file_aligned_with_requested_strategy() {
    let state = app_state(Config::default());
    let first = state.clone();
    let second = state.clone();

    let (first_result, second_result) = tokio::join!(
        apply_node_select(&first, NodeSelect::Fastest(Region::Jp)),
        apply_node_select(&second, NodeSelect::Fastest(Region::Sg)),
    );
    first_result.unwrap();
    second_result.unwrap();

    let requested = *state.node_select_preference.read().await;
    let persisted = tokio::fs::read_to_string(&state.runtime_paths.node_select_preference)
        .await
        .unwrap();
    assert_eq!(persisted.trim(), requested.as_str());
    let _ = tokio::fs::remove_dir_all(&state.runtime_paths.runtime_dir).await;
    let _ = tokio::fs::remove_file(&state.config_path).await;
    let _ = tokio::fs::remove_file(&state.volatile_path).await;
}

#[tokio::test]
async fn node_selection_rejects_unwritable_preference_before_runtime_change() {
    let root = std::env::temp_dir().join(format!(
        "miao-node-select-write-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let config_path = root.join("config.yaml");
    let volatile_path = root.join("volatile.yaml");
    let preference_path = root.join("preference-is-a-directory");
    tokio::fs::create_dir_all(&preference_path).await.unwrap();
    let paths = crate::paths::RuntimePaths::new(root.join("runtime"), &config_path)
        .with_preferences(root.join(".last_proxy"), preference_path);
    let config = Config::default();
    let state = std::sync::Arc::new(
        crate::state::AppState::with_config_layers(
            crate::models::StableConfig::from(&config),
            config,
            config_path,
            volatile_path,
            paths,
        )
        .unwrap(),
    );

    let result = apply_node_select(&state, NodeSelect::Fastest(Region::Jp)).await;

    assert!(result.is_err());
    assert_eq!(state.config.read().await.node_select, NodeSelect::Manual);
    assert_eq!(
        *state.node_select_preference.read().await,
        NodeSelect::Manual
    );
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn failed_runtime_apply_restores_previous_node_selection_preference() {
    let state = app_state(Config {
        nodes: vec![serde_json::json!({
            "type": "hysteria2",
            "tag": "manual-node",
            "server": "192.0.2.1",
            "server_port": 443,
            "password": "secret"
        })
        .to_string()],
        ..Config::default()
    });
    // A directory at the bindings path makes the transaction fail after the
    // new preference has been saved but before runtime activation begins.
    tokio::fs::create_dir_all(&state.runtime_paths.node_bindings)
        .await
        .unwrap();

    let result = apply_node_select(&state, NodeSelect::Fastest(Region::Jp)).await;

    assert!(result.is_err());
    assert_eq!(state.config.read().await.node_select, NodeSelect::Manual);
    assert_eq!(
        *state.node_select_preference.read().await,
        NodeSelect::Manual
    );
    assert!(!state.runtime_paths.node_select_preference.exists());
    let _ = tokio::fs::remove_dir_all(&state.runtime_paths.node_bindings).await;
    let _ = tokio::fs::remove_dir_all(&state.runtime_paths.runtime_dir).await;
    let _ = tokio::fs::remove_file(&state.config_path).await;
    let _ = tokio::fs::remove_file(&state.volatile_path).await;
}

#[tokio::test]
async fn disabled_nodes_mutation_observes_latest_config_under_lock() {
    // RMW 闭包必须基于锁内最新配置计算（并发安全的关键）：预置条目 x，
    // 闭包观察到的禁用集必须包含 x，而不是锁外快照
    let config = Config {
        disabled_nodes: vec![DisabledNode {
            sub: "https://example.com/a".to_string(),
            name: "x".to_string(),
        }],
        ..Config::default()
    };
    let state = app_state(config);

    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_in_closure = observed.clone();
    let result = apply_disabled_nodes(&state, move |config| {
        observed_in_closure
            .lock()
            .unwrap()
            .push(config.disabled_nodes.clone());
        // 拒绝提交：不进入配置事务
        Err("rejected by test".to_string())
    })
    .await;

    assert!(matches!(result, Err(ConfigMutationError::Rejected(_))));
    let seen = observed.lock().unwrap().clone();
    drop(observed);
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].len(), 1);
    assert_eq!(seen[0][0].name, "x");
    // 拒绝后运行配置未被污染
    assert_eq!(state.config.read().await.disabled_nodes.len(), 1);
}

#[tokio::test]
async fn disabled_nodes_noop_mutation_skips_transaction() {
    // 变更后配置无变化 → 幂等早退，不落盘不触内核
    let state = app_state(Config::default());

    let update = apply_disabled_nodes(&state, |_| Ok(())).await.unwrap();

    assert_eq!(update, RuntimeUpdate::None);
}
