use std::{os::unix::fs::PermissionsExt, sync::atomic::Ordering, sync::Arc};

use crate::{
    models::{Config, StableConfig},
    paths::RuntimePaths,
    services::singbox::{is_sing_box_running, start_sing_internal, stop_sing_internal},
    state::AppState,
};

use super::{
    apply_config_change, regenerate_preserving_service_state, regenerate_without_restart_runtime,
    RuntimeUpdate, SubSource,
};

fn manual_node(tag: &str) -> String {
    serde_json::json!({
        "type": "hysteria2",
        "tag": tag,
        "server": "127.0.0.1",
        "server_port": 443,
        "password": "secret"
    })
    .to_string()
}

#[tokio::test]
async fn persistent_save_failure_reactivates_previous_runtime_and_restores_bindings() {
    let unique = format!(
        "miao-transaction-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let runtime_dir = root.join("runtime");
    let config_path = root.join("config.yaml");
    let volatile_path = root.join("volatile.yaml");
    tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
    // A directory at the target path deterministically makes the stable
    // config commit fail after runtime activation, even when tests run as root.
    tokio::fs::create_dir_all(&config_path).await.unwrap();

    let fake_kernel = runtime_dir.join("sing-box");
    tokio::fs::write(
            &fake_kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
    std::fs::set_permissions(&fake_kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

    let old_config = Config {
        nodes: vec![manual_node("old-node")],
        ..Config::default()
    };
    let runtime_paths = RuntimePaths::new(runtime_dir, &config_path);
    let state = Arc::new(
        AppState::with_config_layers(
            StableConfig::from(&old_config),
            old_config.clone(),
            config_path,
            volatile_path,
            runtime_paths,
        )
        .unwrap(),
    );
    let old_runtime = br#"{"marker":"old-runtime"}"#;
    let old_bindings = br#"{"marker":"old-bindings"}"#;
    tokio::fs::write(&state.runtime_paths.active_config, old_runtime)
        .await
        .unwrap();
    tokio::fs::write(&state.runtime_paths.node_bindings, old_bindings)
        .await
        .unwrap();
    start_sing_internal(&state).await.unwrap();
    assert_eq!(state.sing_generation.load(Ordering::Relaxed), 1);
    let original_pid = state
        .sing_process
        .lock()
        .await
        .as_ref()
        .and_then(|process| process.child.id())
        .unwrap();

    let mut new_config = old_config.clone();
    new_config.nodes.push(manual_node("new-node"));
    let result = apply_config_change(&state, &old_config, &new_config).await;

    assert!(result.is_err(), "the persistent config commit must fail");
    assert_eq!(
        tokio::fs::read(&state.runtime_paths.active_config)
            .await
            .unwrap(),
        old_runtime
    );
    assert_eq!(
        tokio::fs::read(&state.runtime_paths.node_bindings)
            .await
            .unwrap(),
        old_bindings
    );
    assert!(is_sing_box_running(&state).await);
    assert_eq!(
        state
            .sing_process
            .lock()
            .await
            .as_ref()
            .and_then(|process| process.child.id()),
        Some(original_pid),
        "Unix rollback should reactivate the previous config without replacing the process"
    );
    assert!(
        state.sing_generation.load(Ordering::Relaxed) >= 3,
        "both activation and rollback must retire their previous watchers"
    );
    assert_eq!(*state.config.read().await, old_config);

    stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn unchanged_runtime_bytes_still_start_a_missing_desired_process() {
    let unique = format!(
        "miao-unchanged-start-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let runtime_dir = root.join("runtime");
    let config_path = root.join("config.yaml");
    let volatile_path = root.join("volatile.yaml");
    tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
    let fake_kernel = runtime_dir.join("sing-box");
    tokio::fs::write(
            &fake_kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
    std::fs::set_permissions(&fake_kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

    let config = Config {
        nodes: vec![manual_node("only-node")],
        ..Config::default()
    };
    let runtime_paths = RuntimePaths::new(runtime_dir, &config_path);
    let state = Arc::new(
        AppState::with_config_layers(
            StableConfig::from(&config),
            config.clone(),
            config_path,
            volatile_path,
            runtime_paths,
        )
        .unwrap(),
    );

    regenerate_without_restart_runtime(&config, &state, SubSource::Fetch)
        .await
        .unwrap();
    assert!(!is_sing_box_running(&state).await);

    let runtime_update = regenerate_preserving_service_state(&config, &state)
        .await
        .unwrap();

    assert_eq!(runtime_update, RuntimeUpdate::Started);
    assert!(is_sing_box_running(&state).await);
    assert!(state.runtime_ready.load(Ordering::Relaxed));

    stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn config_update_lock_serializes_overlapping_node_adds() {
    let unique = format!(
        "miao-serialize-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let root = std::env::temp_dir().join(unique);
    let runtime_dir = root.join("runtime");
    let config_path = root.join("config.yaml");
    let volatile_path = root.join("volatile.yaml");
    tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
    let fake_kernel = runtime_dir.join("sing-box");
    tokio::fs::write(
            &fake_kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
    std::fs::set_permissions(&fake_kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

    let old_config = Config {
        nodes: vec![manual_node("base-node")],
        ..Config::default()
    };
    let runtime_paths = RuntimePaths::new(runtime_dir, &config_path);
    let state = Arc::new(
        AppState::with_config_layers(
            StableConfig::from(&old_config),
            old_config.clone(),
            config_path,
            volatile_path,
            runtime_paths,
        )
        .unwrap(),
    );
    start_sing_internal(&state).await.unwrap();

    let add = |state: Arc<AppState>, tag: &'static str| async move {
        let _guard = state.config_update.lock().await;
        let old = state.config.read().await.clone();
        let mut new = old.clone();
        new.nodes.push(manual_node(tag));
        apply_config_change(&state, &old, &new).await
    };

    let (first, second) = tokio::join!(add(state.clone(), "node-a"), add(state.clone(), "node-b"),);
    first.expect("first add should apply");
    second.expect("second add should apply");

    let tags: Vec<String> = state
        .config
        .read()
        .await
        .nodes
        .iter()
        .filter_map(|raw| {
            serde_json::from_str::<serde_json::Value>(raw)
                .ok()?
                .get("tag")?
                .as_str()
                .map(str::to_string)
        })
        .collect();
    assert!(tags.contains(&"base-node".to_string()));
    assert!(tags.contains(&"node-a".to_string()));
    assert!(tags.contains(&"node-b".to_string()));
    assert_eq!(tags.len(), 3);

    stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}
