use super::*;

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
