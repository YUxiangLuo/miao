use std::path::PathBuf;

use super::{panel_bind_addr, prepare_compatible_startup_cache, spawn_server, RuntimeOptions};
// 仅被 #[cfg(unix)] 测试使用（Windows 测试构建下 import 会报未使用）。
#[cfg(unix)]
use super::{initialize_runtime_locked, recover_data_plane_once};

#[tokio::test]
async fn incompatible_cache_is_rejected_before_it_replaces_active_config() {
    use crate::{models::Config, services::config::save_config_cache, test_support::app_state};

    let original = Config {
        subs: vec!["https://old.example/sub".to_string()],
        ..Config::default()
    };
    let state = app_state(original);
    tokio::fs::create_dir_all(&state.runtime_paths.runtime_dir)
        .await
        .unwrap();
    tokio::fs::write(&state.runtime_paths.active_config, br#"{"outbounds":[]}"#)
        .await
        .unwrap();
    save_config_cache(&state).await;

    let active_before_fallback = br#"{"marker":"active-before-fallback"}"#;
    tokio::fs::write(&state.runtime_paths.active_config, active_before_fallback)
        .await
        .unwrap();
    let changed = Config {
        subs: vec!["https://new.example/sub".to_string()],
        ..Config::default()
    };

    let result = prepare_compatible_startup_cache(&changed, &state).await;

    assert!(result.is_err());
    assert_eq!(
        tokio::fs::read(&state.runtime_paths.active_config)
            .await
            .unwrap(),
        active_before_fallback
    );
    let _ = tokio::fs::remove_dir_all(&state.runtime_paths.runtime_dir).await;
}

#[cfg(unix)]
async fn local_startup_test_state(
    config: crate::models::Config,
    label: &str,
) -> (std::sync::Arc<crate::state::AppState>, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let root = std::env::temp_dir().join(format!(
        "miao-local-startup-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let runtime_dir = root.join("runtime");
    tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
    let kernel = runtime_dir.join("sing-box");
    tokio::fs::write(
            &kernel,
            b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n",
        )
        .await
        .unwrap();
    std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

    let config_path = root.join("config.yaml");
    let volatile_path = root.join("volatile.yaml");
    let paths = crate::paths::RuntimePaths::new(runtime_dir, &config_path);
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
    (state, root)
}

#[cfg(unix)]
async fn subscription_server(
    accepted: Option<std::sync::Arc<tokio::sync::Notify>>,
    release: Option<std::sync::Arc<tokio::sync::Notify>>,
) -> String {
    use axum::{routing::get, Router};

    const BODY: &str = r#"
proxies:
  - name: recovered-sub-node
    type: hysteria2
    server: 127.0.0.1
    port: 443
    password: secret
"#;
    let app = Router::new().route(
        "/sub",
        get(move || {
            let accepted = accepted.clone();
            let release = release.clone();
            async move {
                if let Some(accepted) = accepted {
                    accepted.notify_one();
                }
                if let Some(release) = release {
                    release.notified().await;
                }
                BODY
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}/sub")
}

#[cfg(unix)]
async fn eventually_available_subscription_server(
    failures: usize,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::{http::StatusCode, routing::get, Router};
    use std::sync::atomic::Ordering;

    const BODY: &str = r#"
proxies:
  - name: refreshed-sub-node
    type: hysteria2
    server: 127.0.0.1
    port: 443
    password: secret
"#;
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_handler = calls.clone();
    let app = Router::new().route(
        "/sub",
        get(move || {
            let calls = calls_for_handler.clone();
            async move {
                let attempt = calls.fetch_add(1, Ordering::Relaxed) + 1;
                let status = if attempt <= failures {
                    StatusCode::SERVICE_UNAVAILABLE
                } else {
                    StatusCode::OK
                };
                (status, BODY)
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/sub"), calls)
}

#[cfg(unix)]
async fn superseding_subscription_server(
    first_accepted: std::sync::Arc<tokio::sync::Notify>,
    release_first: std::sync::Arc<tokio::sync::Notify>,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::{http::StatusCode, routing::get, Router};
    use std::sync::atomic::Ordering;

    const BODY: &str = r#"
proxies:
  - name: recovered-after-foreground-failure
    type: hysteria2
    server: 127.0.0.1
    port: 443
    password: secret
"#;
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls_for_handler = calls.clone();
    let app = Router::new().route(
        "/sub",
        get(move || {
            let calls = calls_for_handler.clone();
            let first_accepted = first_accepted.clone();
            let release_first = release_first.clone();
            async move {
                let attempt = calls.fetch_add(1, Ordering::Relaxed) + 1;
                match attempt {
                    1 => {
                        first_accepted.notify_one();
                        release_first.notified().await;
                        (StatusCode::SERVICE_UNAVAILABLE, BODY)
                    }
                    2 => (StatusCode::SERVICE_UNAVAILABLE, BODY),
                    _ => (StatusCode::OK, BODY),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/sub"), calls)
}

#[cfg(unix)]
async fn refusing_sub_url() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            drop(socket);
        }
    });
    format!("http://{addr}/sub")
}

#[cfg(unix)]
#[tokio::test]
async fn manual_nodes_start_before_any_subscription_request() {
    let config = crate::models::Config {
        subs: vec!["http://127.0.0.1:9/unreachable".to_string()],
        nodes: vec![serde_json::json!({
            "type": "hysteria2",
            "tag": "manual-local",
            "server": "127.0.0.1",
            "server_port": 443,
            "password": "secret"
        })
        .to_string()],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config.clone(), "manual").await;

    let needs_refresh = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        initialize_runtime_locked(&config, &state),
    )
    .await
    .expect("local startup must not wait for the unreachable subscription");

    assert!(needs_refresh);
    assert!(state
        .runtime_ready
        .load(std::sync::atomic::Ordering::Relaxed));
    assert!(state.sub_status.lock().await.is_empty());
    assert!(state.runtime_paths.config_cache.exists());

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn legacy_manual_cache_requests_local_background_reconciliation() {
    let config = crate::models::Config {
        nodes: vec![serde_json::json!({
            "type": "hysteria2",
            "tag": "legacy-manual",
            "server": "127.0.0.1",
            "server_port": 443,
            "password": "secret"
        })
        .to_string()],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config.clone(), "legacy-manual").await;
    let outcome = crate::services::config::gen_config_from_nodes(&config, &state, Vec::new())
        .await
        .unwrap();
    crate::services::config::install_prepared_runtime(&state, &outcome)
        .await
        .unwrap();
    crate::services::config::save_config_cache(&state).await;
    tokio::fs::remove_file(&state.runtime_paths.cache_manifest)
        .await
        .unwrap();

    let needs_reconciliation = initialize_runtime_locked(&config, &state).await;

    assert!(needs_reconciliation);
    assert!(state
        .runtime_ready
        .load(std::sync::atomic::Ordering::Relaxed));

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn matching_node_snapshot_starts_before_any_subscription_request() {
    let subscription = "http://127.0.0.1:9/unreachable".to_string();
    let config = crate::models::Config {
        subs: vec![subscription.clone()],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config.clone(), "snapshot").await;
    let snapshot = serde_json::json!({
        "version": 1,
        "subs": [subscription],
        "node_names": ["snapshot-local"],
        "outbounds": [{
            "type": "hysteria2",
            "tag": "snapshot-local",
            "server": "127.0.0.1",
            "server_port": 443,
            "password": "secret"
        }],
        "source_ids": ["snapshot-source"]
    });
    tokio::fs::write(
        &state.runtime_paths.sub_nodes_snapshot,
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .await
    .unwrap();

    let needs_refresh = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        initialize_runtime_locked(&config, &state),
    )
    .await
    .expect("snapshot startup must not wait for the unreachable subscription");

    assert!(needs_refresh);
    assert!(state
        .runtime_ready
        .load(std::sync::atomic::Ordering::Relaxed));
    assert!(state.sub_status.lock().await.is_empty());
    let active = tokio::fs::read_to_string(&state.runtime_paths.active_config)
        .await
        .unwrap();
    assert!(active.contains("snapshot-local"));

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn manual_node_startup_keeps_retrying_subscriptions_after_the_initial_budget() {
    use std::sync::atomic::Ordering;

    // 测试启动预算共请求三次（首次 + 两次短退避）；第四次只有外层持续
    // 重试存在时才会发生。模拟 /tmp 已随重启清空，但 config.yaml 仍有手动节点。
    let (subscription, calls) = eventually_available_subscription_server(3).await;
    let config = crate::models::Config {
        subs: vec![subscription.clone()],
        nodes: vec![serde_json::json!({
            "type": "hysteria2",
            "tag": "manual-startup-node",
            "server": "192.0.2.1",
            "server_port": 8443,
            "password": "secret"
        })
        .to_string()],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config.clone(), "manual-refresh-retry").await;

    assert!(!state.runtime_paths.config_cache.exists());
    assert!(!state.runtime_paths.sub_nodes_snapshot.exists());
    assert!(initialize_runtime_locked(&config, &state).await);
    assert!(state.runtime_ready.load(Ordering::Relaxed));

    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        super::refresh_subscriptions_in_background(&config, &state),
    )
    .await
    .expect("manual-node startup refresh must recover after its initial retry budget");

    assert!(calls.load(Ordering::Relaxed) >= 4);
    assert!(state.runtime_ready.load(Ordering::Relaxed));
    assert_eq!(state.runtime_phase(), crate::models::RuntimePhase::Ready);
    assert!(state.config_warning.lock().await.is_none());
    assert_eq!(
        state
            .sub_status
            .lock()
            .await
            .get(&subscription)
            .map(|status| (status.success, status.node_count)),
        Some((true, 1))
    );
    let refreshed_snapshot = tokio::fs::read_to_string(&state.runtime_paths.sub_nodes_snapshot)
        .await
        .unwrap();
    assert!(refreshed_snapshot.contains("refreshed-sub-node"));

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn failed_foreground_refresh_does_not_cancel_startup_subscription_recovery() {
    use std::sync::atomic::Ordering;

    let first_accepted = std::sync::Arc::new(tokio::sync::Notify::new());
    let release_first = std::sync::Arc::new(tokio::sync::Notify::new());
    let (subscription, calls) =
        superseding_subscription_server(first_accepted.clone(), release_first.clone()).await;
    let config = crate::models::Config {
        subs: vec![subscription.clone()],
        nodes: vec![serde_json::json!({
            "type": "hysteria2",
            "tag": "manual-during-foreground-refresh",
            "server": "192.0.2.2",
            "server_port": 8443,
            "password": "secret"
        })
        .to_string()],
        ..crate::models::Config::default()
    };
    let (state, root) =
        local_startup_test_state(config.clone(), "foreground-refresh-failure").await;
    assert!(initialize_runtime_locked(&config, &state).await);

    let background_state = state.clone();
    let background_config = config.clone();
    let background = tokio::spawn(async move {
        super::refresh_subscriptions_in_background(&background_config, &background_state).await;
    });
    first_accepted.notified().await;

    // 模拟用户在第一次启动刷新尚未返回时点击手动刷新。第二个请求失败；因为
    // 手动节点仍可生成配置，HTTP handler 会完成，但没有获取到任何订阅节点。
    let config_update = state.config_update.lock().await;
    crate::services::config::regenerate_preserving_service_state(&config, &state)
        .await
        .unwrap();
    drop(config_update);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        state
            .sub_status
            .lock()
            .await
            .get(&subscription)
            .map(|status| status.success),
        Some(false)
    );

    release_first.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(3), background)
        .await
        .expect("startup recovery must continue after a failed foreground fetch")
        .unwrap();

    assert!(calls.load(Ordering::Relaxed) >= 3);
    assert_eq!(
        state
            .sub_status
            .lock()
            .await
            .get(&subscription)
            .map(|status| (status.success, status.node_count)),
        Some((true, 1))
    );

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn local_startup_does_not_fetch_subscriptions_under_the_config_lock() {
    let accepted = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let subscription = subscription_server(Some(accepted.clone()), Some(release.clone())).await;
    let config = crate::models::Config {
        subs: vec![subscription],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config.clone(), "no-lock-fetch").await;

    let finished = tokio::time::timeout(
        std::time::Duration::from_millis(400),
        initialize_runtime_locked(&config, &state),
    )
    .await
    .expect("locked local startup must return without waiting on subscription HTTP");

    assert!(!finished);
    assert!(!state
        .runtime_ready
        .load(std::sync::atomic::Ordering::Relaxed));
    // The hung subscription server was never contacted.
    tokio::time::timeout(std::time::Duration::from_millis(50), accepted.notified())
        .await
        .expect_err("subscription fetch must not run under the config lock");

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn failed_initial_start_keeps_retrying_until_the_data_plane_recovers() {
    use std::sync::atomic::Ordering;

    let subscription = subscription_server(None, None).await;
    let config = crate::models::Config {
        subs: vec![subscription],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config, "retry-recovery").await;
    state.initializing.store(false, Ordering::Relaxed);
    state.set_runtime_phase(crate::models::RuntimePhase::Failed);

    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        super::retry_failed_startup(&state),
    )
    .await
    .expect("background startup recovery must succeed");

    assert!(state.runtime_ready.load(Ordering::Relaxed));
    assert_eq!(state.runtime_phase(), crate::models::RuntimePhase::Ready);
    assert!(state.runtime_paths.config_cache.exists());
    assert_eq!(
        state
            .sub_status
            .lock()
            .await
            .values()
            .next()
            .map(|status| status.node_count),
        Some(1)
    );

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_starts_compatible_cache_when_subscription_fetch_fails() {
    use std::sync::atomic::Ordering;

    let subscription = refusing_sub_url().await;
    let config = crate::models::Config {
        subs: vec![subscription.clone()],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config.clone(), "recover-cache").await;
    let snapshot = serde_json::json!({
        "version": 1,
        "subs": [subscription],
        "node_names": ["cached-sub-node"],
        "outbounds": [{
            "type": "hysteria2",
            "tag": "cached-sub-node",
            "server": "127.0.0.1",
            "server_port": 443,
            "password": "secret"
        }],
        "source_ids": ["cached-source"]
    });
    tokio::fs::write(
        &state.runtime_paths.sub_nodes_snapshot,
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .await
    .unwrap();

    assert!(initialize_runtime_locked(&config, &state).await);
    assert!(state.runtime_ready.load(Ordering::Relaxed));
    crate::services::singbox::stop_sing_internal(&state).await;
    state.runtime_ready.store(false, Ordering::Relaxed);
    state.set_runtime_phase(crate::models::RuntimePhase::Failed);
    let _ = tokio::fs::remove_file(&state.runtime_paths.sub_nodes_snapshot).await;

    let recovered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        recover_data_plane_once(&state),
    )
    .await
    .expect("cache recovery must finish");

    assert!(recovered);
    assert!(state.runtime_ready.load(Ordering::Relaxed));
    let active = tokio::fs::read_to_string(&state.runtime_paths.active_config)
        .await
        .unwrap();
    assert!(active.contains("cached-sub-node"));

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_activates_manuals_when_fetch_fails_and_nothing_is_running() {
    use std::sync::atomic::Ordering;

    let subscription = refusing_sub_url().await;
    let config = crate::models::Config {
        subs: vec![subscription],
        nodes: vec![serde_json::json!({
            "type": "hysteria2",
            "tag": "manual-recover",
            "server": "127.0.0.1",
            "server_port": 443,
            "password": "secret"
        })
        .to_string()],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config, "recover-manuals").await;
    state.initializing.store(false, Ordering::Relaxed);
    state.set_runtime_phase(crate::models::RuntimePhase::Failed);

    let recovered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        recover_data_plane_once(&state),
    )
    .await
    .expect("manual recovery must finish");

    assert!(recovered);
    assert!(state.runtime_ready.load(Ordering::Relaxed));
    let active = tokio::fs::read_to_string(&state.runtime_paths.active_config)
        .await
        .unwrap();
    assert!(active.contains("manual-recover"));

    crate::services::singbox::stop_sing_internal(&state).await;
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(unix)]
#[tokio::test]
async fn foreground_refresh_supersedes_an_older_startup_fetch() {
    use std::sync::atomic::Ordering;

    let accepted = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let subscription = subscription_server(Some(accepted.clone()), Some(release.clone())).await;
    let config = crate::models::Config {
        subs: vec![subscription.clone()],
        ..crate::models::Config::default()
    };
    let (state, root) = local_startup_test_state(config.clone(), "refresh-generation").await;
    let active_before = br#"{"marker":"foreground-runtime"}"#;
    tokio::fs::write(&state.runtime_paths.active_config, active_before)
        .await
        .unwrap();
    state.runtime_ready.store(true, Ordering::Relaxed);
    state.set_runtime_phase(crate::models::RuntimePhase::Ready);

    let background_state = state.clone();
    let background = tokio::spawn(async move {
        super::refresh_subscriptions_in_background(&config, &background_state).await;
    });
    accepted.notified().await;

    let foreground_generation = state.sub_refresh_generation.fetch_add(1, Ordering::Relaxed) + 1;
    state
        .sub_refresh_success_generation
        .store(foreground_generation, Ordering::Relaxed);
    state.sub_status.lock().await.insert(
        subscription.clone(),
        crate::models::SubStatus {
            url: subscription,
            success: true,
            node_count: 99,
            disabled_count: 0,
            state: crate::models::SubscriptionState::Ready,
            error: None,
        },
    );
    state.set_runtime_phase(crate::models::RuntimePhase::Ready);
    release.notify_one();
    background.await.unwrap();

    assert_eq!(
        tokio::fs::read(&state.runtime_paths.active_config)
            .await
            .unwrap(),
        active_before
    );
    assert_eq!(
        state
            .sub_status
            .lock()
            .await
            .values()
            .next()
            .map(|status| status.node_count),
        Some(99)
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

/// Hold a port the panel would bind. Windows needs SO_EXCLUSIVEADDRUSE so
/// Tokio's SO_REUSEADDR cannot hijack it.
fn occupy_panel_port() -> (std::net::TcpListener, u16) {
    let probe = std::net::TcpListener::bind(panel_bind_addr(0)).expect("probe port");
    let port = probe.local_addr().expect("probe addr").port();
    drop(probe);

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{
            setsockopt, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
        };

        let socket = tokio::net::TcpSocket::new_v4().expect("tcp socket");
        let enable: i32 = 1;
        let rc = unsafe {
            setsockopt(
                socket.as_raw_socket() as _,
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                (&enable as *const i32).cast(),
                std::mem::size_of_val(&enable) as i32,
            )
        };
        assert_ne!(rc, -1, "SO_EXCLUSIVEADDRUSE");
        socket
            .bind(panel_bind_addr(port).parse().expect("addr"))
            .expect("exclusive bind");
        let listener = socket
            .listen(1)
            .expect("listen")
            .into_std()
            .expect("into std listener");
        (listener, port)
    }

    #[cfg(not(windows))]
    {
        let listener = std::net::TcpListener::bind(panel_bind_addr(port)).expect("occupy port");
        (listener, port)
    }
}

#[test]
fn resolve_log_path_honors_explicit_override() {
    let path = PathBuf::from("/tmp/miao-test.log");
    let options = RuntimeOptions {
        log_path: Some(path.clone()),
        ..RuntimeOptions::default()
    };
    assert_eq!(super::resolve_log_path(&options), Some(path));
}

#[tokio::test]
async fn spawn_server_serves_status_and_shuts_down() {
    let config_path = unique_test_config_path();

    let handle = spawn_server(RuntimeOptions {
        open_browser: false,
        install_tracing: false,
        bind_port: Some(0),
        port_fallback: false,
        config_path: Some(config_path),
        volatile_path: Some(unique_test_volatile_path()),
        skip_extract: true,
        runtime_dir: Some(unique_test_runtime_dir()),
        log_path: None,
    })
    .await
    .expect("spawn panel");

    assert!(handle.url().starts_with("http://127.0.0.1:"));
    assert_ne!(handle.port(), 0);

    let client = reqwest::Client::new();
    let status_url = format!("{}/api/status", handle.url());
    let mut last_error = None;
    let mut body = None;
    for _ in 0..20 {
        match client.get(&status_url).send().await {
            Ok(response) => {
                assert!(response.status().is_success());
                body = Some(response.text().await.expect("status body"));
                last_error = None;
                break;
            }
            Err(err) => {
                last_error = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
    if let Some(err) = last_error {
        panic!("panel did not become ready: {err}");
    }
    let body = body.expect("status body");
    assert!(body.contains("stopped") || body.contains("running"));

    let url = handle.url().to_string();
    handle.shutdown().await;

    let after = client.get(format!("{url}/api/status")).send().await;
    assert!(
        after.is_err(),
        "panel should reject requests after shutdown"
    );
}

#[tokio::test]
async fn spawn_server_falls_back_to_ephemeral_port_when_occupied() {
    let (blocker, occupied_port) = occupy_panel_port();

    let handle = spawn_server(RuntimeOptions {
        open_browser: false,
        install_tracing: false,
        bind_port: Some(occupied_port),
        port_fallback: true,
        config_path: Some(unique_test_config_path()),
        volatile_path: Some(unique_test_volatile_path()),
        skip_extract: true,
        runtime_dir: Some(unique_test_runtime_dir()),
        log_path: None,
    })
    .await
    .expect("spawn panel with port fallback");

    assert_ne!(handle.port(), occupied_port);
    handle.shutdown().await;
    drop(blocker);
}

#[tokio::test]
async fn spawn_server_without_port_fallback_fails_when_occupied() {
    let (blocker, occupied_port) = occupy_panel_port();

    let result = spawn_server(RuntimeOptions {
        open_browser: false,
        install_tracing: false,
        bind_port: Some(occupied_port),
        port_fallback: false,
        config_path: Some(unique_test_config_path()),
        volatile_path: Some(unique_test_volatile_path()),
        skip_extract: true,
        runtime_dir: Some(unique_test_runtime_dir()),
        log_path: None,
    })
    .await;

    assert!(result.is_err());
    drop(blocker);
}

#[test]
fn rotated_log_path_appends_old_suffix() {
    let path = PathBuf::from("/tmp/miao.log");
    assert_eq!(
        super::rotated_log_path(&path),
        PathBuf::from("/tmp/miao.log.old")
    );
}

#[test]
fn rotate_oversized_log_keeps_small_file() {
    let path = unique_test_log_path("small");
    std::fs::write(&path, b"small").expect("write small log");

    super::rotate_oversized_log(&path);

    assert!(path.exists());
    assert!(!super::rotated_log_path(&path).exists());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn rotate_oversized_log_renames_big_file() {
    let path = unique_test_log_path("big");
    let big = vec![b'x'; (super::MAX_LOG_FILE_BYTES + 1) as usize];
    std::fs::write(&path, &big).expect("write big log");

    super::rotate_oversized_log(&path);

    assert!(!path.exists());
    let rotated = super::rotated_log_path(&path);
    assert_eq!(
        std::fs::metadata(&rotated).expect("rotated log").len(),
        super::MAX_LOG_FILE_BYTES + 1
    );
    let _ = std::fs::remove_file(&rotated);
}

fn unique_test_config_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "miao-spawn-server-test-{}-{}.yaml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn unique_test_volatile_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "miao-spawn-server-test-volatile-{}-{}.yaml",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn unique_test_runtime_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "miao-spawn-server-runtime-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn unique_test_log_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "miao-rotate-{tag}-{}-{}.log",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}
