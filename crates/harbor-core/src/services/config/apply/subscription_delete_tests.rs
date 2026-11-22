//! Real subscription transactions with isolated files, loopback HTTP and an
//! ACK-only fake kernel. No data-plane listeners, TUN or production paths are used.
use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
};

use axum::{http::StatusCode, routing::get, Router};
use tokio::sync::Notify;

use crate::{
    models::{Config, StableConfig, SubRequest},
    services::{
        commands::subs::delete_sub,
        config::read_sub_nodes_snapshot,
        singbox::{start_sing_internal, stop_sing_internal},
    },
    state::AppState,
    test_support::{isolated_stopped_state, FAKE_KERNEL},
};

use super::{refresh_subscriptions_foreground, ConfigEdit, ConfigMutationError};

#[derive(Default)]
struct Source {
    // 0 = usable, 1 = HTTP failure, 2 = authoritative empty, 3 = delayed usable.
    mode: AtomicU8,
    requests: AtomicUsize,
    entered: Notify,
    release: Notify,
}

struct Fixture {
    _root: tempfile::TempDir,
    state: Arc<AppState>,
    urls: [String; 3],
    sources: [Arc<Source>; 3],
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn manual_node(detour: Option<&str>) -> String {
    let mut node = serde_json::json!({
        "type": "socks", "tag": "manual", "server": "192.0.2.20",
        "server_port": 1080, "version": "5"
    });
    if let Some(tag) = detour {
        node["detour"] = tag.into();
    }
    node.to_string()
}

impl Fixture {
    async fn new(keep_b: bool, include_c: bool, nodes: Vec<String>, running: bool) -> Self {
        let sources: [Arc<Source>; 3] = std::array::from_fn(|_| Arc::new(Source::default()));
        let mut app = Router::new();
        for (name, source) in ["a", "b", "c"].into_iter().zip(&sources) {
            let source = source.clone();
            app = app.route(&format!("/{name}"), get(move || {
                let source = source.clone();
                async move {
                    source.requests.fetch_add(1, Ordering::Relaxed);
                    match source.mode.load(Ordering::Relaxed) {
                        1 => return (StatusCode::BAD_GATEWAY, "fixture failure".to_string()),
                        2 => return (StatusCode::OK, "proxies: []\n".to_string()),
                        3 => { source.entered.notify_one(); source.release.notified().await; }
                        _ => {}
                    }
                    (StatusCode::OK, format!("proxies:\n- name: node-{name}\n  type: trojan\n  server: {name}.example.com\n  port: 443\n  password: fixture-password\n"))
                }
            }));
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let urls = ["a", "b", "c"].map(|name| format!("{origin}/{name}"));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut config = Config {
            subs: vec![urls[0].clone()],
            nodes,
            ..Config::default()
        };
        if keep_b {
            config.subs.push(urls[1].clone());
        }
        if include_c {
            config.subs.push(urls[2].clone());
        }
        let (root, state) = isolated_stopped_state(config.clone());
        std::fs::write(
            &state.config_path,
            yaml_serde::to_string(&StableConfig::from(&config)).unwrap(),
        )
        .unwrap();
        let kernel = state.runtime_paths.runtime_dir.join("sing-box");
        std::fs::write(&kernel, FAKE_KERNEL).unwrap();
        std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            refresh_subscriptions_foreground(&state)
                .await
                .unwrap()
                .fetch_succeeded
        );
        // Add the rule only after A owns its stable tag; otherwise the binding
        // allocator correctly reserves the rule's missing target for safety.
        let mut edit = ConfigEdit::begin(&state).await;
        edit.candidate
            .custom_rules
            .push(r#"{"domain_suffix":"deleted.example","outbound":"node-a"}"#.into());
        edit.commit().await.unwrap();
        let runtime: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&state.runtime_paths.active_config).unwrap())
                .unwrap();
        assert!(runtime["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["tag"] == "node-a"));
        assert!(runtime["route"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule["outbound"] == "node-a"));
        if running {
            state.lifecycle.request_running(true);
            start_sing_internal(&state).await.unwrap();
        }
        Self {
            _root: root,
            state,
            urls,
            sources,
            server,
        }
    }

    async fn discard_snapshot(&self) {
        std::fs::remove_file(&self.state.runtime_paths.sub_nodes_snapshot).unwrap();
        *self.state.sub_nodes_cache.write().await = None;
    }

    async fn delete_a(&self) -> crate::services::commands::CommandResult {
        delete_sub(
            self.state.clone(),
            SubRequest {
                url: self.urls[0].clone(),
            },
        )
        .await
    }

    fn runtime(&self) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(&self.state.runtime_paths.active_config).unwrap())
            .unwrap()
    }

    async fn assert_removed(&self, has_runtime: bool) {
        let current = self.state.config.read().await.clone();
        let persisted: StableConfig =
            yaml_serde::from_slice(&std::fs::read(&self.state.config_path).unwrap()).unwrap();
        assert_eq!(current.subs, persisted.subs);
        assert!(!current.subs.contains(&self.urls[0]));
        assert!(!self
            .state
            .sub_status
            .lock()
            .await
            .contains_key(&self.urls[0]));
        if has_runtime {
            let runtime = self.runtime();
            assert!(!runtime["outbounds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["tag"] == "node-a"));
            assert!(
                runtime["route"]["rules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|rule| rule["domain_suffix"] == "deleted.example"
                        && rule["action"] == "reject"),
                "Rules targeting the deleted node must block, never fall back"
            );
        } else {
            assert!(!self.state.runtime_paths.active_config.exists());
            assert!(!self.state.runtime_paths.config_cache.exists());
            assert!(self.state.sing_process.lock().await.is_none());
        }
        // An empty runtime still keeps remaining sources' canonical material,
        // including authoritative empty lists and disabled nodes. Re-read from
        // disk to verify its fingerprint, not just the panel's memory cache.
        *self.state.sub_nodes_cache.write().await = None;
        let snapshot = read_sub_nodes_snapshot(&self.state).await;
        if has_runtime || !current.subs.is_empty() {
            let snapshot = snapshot.unwrap();
            assert!(snapshot.matches_subs(&current.subs));
            assert!(!snapshot.node_names.iter().any(|name| name == "node-a"));
        } else {
            assert!(snapshot.is_none());
            assert!(!self.state.runtime_paths.sub_nodes_snapshot.exists());
        }
    }
}

#[tokio::test]
async fn subscription_delete_availability_matrix() {
    // label, B mode, cache, manual, C mode, keep B, accept, runtime remains
    let cases = [
        (
            "total_failure_no_fallback",
            1,
            false,
            false,
            None,
            true,
            false,
            true,
        ),
        (
            "failure_with_retained_cache",
            1,
            true,
            false,
            None,
            true,
            true,
            true,
        ),
        (
            "failure_with_manual",
            1,
            false,
            true,
            None,
            true,
            true,
            true,
        ),
        (
            "partial_failure_with_usable_source",
            1,
            false,
            false,
            Some(0),
            true,
            true,
            true,
        ),
        ("successful_empty", 2, false, false, None, true, true, false),
        (
            "successful_empty_with_manual",
            2,
            false,
            true,
            None,
            true,
            true,
            true,
        ),
        (
            "partial_failure_with_empty_source",
            1,
            false,
            false,
            Some(2),
            true,
            true,
            false,
        ),
        (
            "deletion_does_not_refresh_cached_sources",
            2,
            true,
            false,
            None,
            true,
            true,
            true,
        ),
        ("usable_response", 0, false, false, None, true, true, true),
        (
            "last_subscription",
            1,
            true,
            false,
            None,
            false,
            true,
            false,
        ),
        (
            "last_subscription_with_manual",
            1,
            true,
            true,
            None,
            false,
            true,
            true,
        ),
    ];
    for running in [false, true] {
        for (label, b_mode, cache, manual, c_mode, keep_b, accept, has_runtime) in cases {
            let nodes = if manual {
                vec![manual_node(None)]
            } else {
                vec![]
            };
            let fixture = Fixture::new(keep_b, c_mode.is_some(), nodes, running).await;
            if !cache {
                fixture.discard_snapshot().await;
            }
            fixture.sources[1].mode.store(b_mode, Ordering::Relaxed);
            if let Some(mode) = c_mode {
                fixture.sources[2].mode.store(mode, Ordering::Relaxed);
            }
            let old_runtime = std::fs::read(&fixture.state.runtime_paths.active_config).unwrap();
            let old_config = std::fs::read(&fixture.state.config_path).unwrap();
            for _ in 0..if accept { 1 } else { 2 } {
                let result = fixture.delete_a().await;
                assert_eq!(
                    result.is_ok(),
                    accept,
                    "{label} running={running}: {result:?}"
                );
                assert_eq!(fixture.sources[0].requests.load(Ordering::Relaxed), 1);
                if accept {
                    fixture.assert_removed(has_runtime).await;
                    if has_runtime {
                        assert_eq!(fixture.state.sing_process.lock().await.is_some(), running);
                    }
                } else {
                    assert_eq!(
                        std::fs::read(&fixture.state.runtime_paths.active_config).unwrap(),
                        old_runtime
                    );
                    assert_eq!(
                        std::fs::read(&fixture.state.config_path).unwrap(),
                        old_config
                    );
                    assert!(fixture
                        .state
                        .config
                        .read()
                        .await
                        .subs
                        .contains(&fixture.urls[0]));
                    assert!(result.unwrap_err().to_string().contains("没有可用节点"));
                }
            }
            if cache && keep_b {
                assert_eq!(
                    fixture.sources[1].requests.load(Ordering::Relaxed),
                    1,
                    "{label}: deletion must not fetch retained sources"
                );
                // Later failures and local disk reloads cannot resurrect A.
                fixture.sources[1].mode.store(1, Ordering::Relaxed);
                let refresh = refresh_subscriptions_foreground(&fixture.state)
                    .await
                    .unwrap();
                assert!(!refresh.fetch_succeeded);
                fixture.assert_removed(true).await;
            }
            stop_sing_internal(&fixture.state).await;
        }
    }
}

#[tokio::test]
async fn subscription_delete_isolates_missing_detours_and_keeps_valid_fallbacks() {
    for running in [false, true] {
        for cache in [false, true] {
            for independent_manual in [false, true] {
                let mut nodes = vec![manual_node(Some("node-a"))];
                if independent_manual {
                    let mut node: serde_json::Value =
                        serde_json::from_str(&manual_node(None)).unwrap();
                    node["tag"] = "independent".into();
                    nodes.push(node.to_string());
                }
                let fixture = Fixture::new(true, false, nodes.clone(), running).await;
                let mut edit = ConfigEdit::begin(&fixture.state).await;
                edit.candidate
                    .custom_rules
                    .push(r#"{"source_ip_cidr":"192.168.8.194/32","outbound":"manual"}"#.into());
                edit.commit().await.unwrap();
                if !cache {
                    fixture.discard_snapshot().await;
                }
                fixture.delete_a().await.unwrap();
                fixture.assert_removed(true).await;
                let runtime = fixture.runtime();
                let outbounds = runtime["outbounds"].as_array().unwrap();
                assert!(outbounds.iter().any(|node| node["tag"] == "node-b"));
                assert!(!outbounds.iter().any(|node| node["tag"] == "manual"));
                assert_eq!(
                    outbounds.iter().any(|node| node["tag"] == "independent"),
                    independent_manual
                );
                for section in ["route", "dns"] {
                    assert!(
                        runtime[section]["rules"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|rule| rule["source_ip_cidr"] == "192.168.8.194/32"
                                && rule["action"] == "reject"),
                        "{section} must block the missing employee egress"
                    );
                }
                assert_eq!(fixture.state.config.read().await.nodes, nodes);
                assert_eq!(
                    fixture.sources[1].requests.load(Ordering::Relaxed),
                    if cache { 1 } else { 2 }
                );
                assert_eq!(fixture.state.sing_process.lock().await.is_some(), running);

                // Repair the retained manual input locally: no fetch is needed
                // to restore the node and its source-IP/DNS bindings.
                let requests = fixture.sources[1].requests.load(Ordering::Relaxed);
                let mut edit = ConfigEdit::begin(&fixture.state).await;
                edit.candidate.nodes[0] = manual_node(Some("node-b"));
                edit.commit().await.unwrap();
                let restored = fixture.runtime();
                assert!(restored["outbounds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|node| node["tag"] == "manual" && node["detour"] == "node-b"));
                assert!(restored["route"]["rules"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|rule| rule["source_ip_cidr"] == "192.168.8.194/32"
                        && rule["outbound"] == "manual"));
                assert_eq!(
                    fixture.sources[1].requests.load(Ordering::Relaxed),
                    requests
                );
                stop_sing_internal(&fixture.state).await;
            }
        }
    }
}

#[tokio::test]
async fn subscription_delete_supersedes_a_pending_refresh_without_resurrection() {
    let fixture = Fixture::new(true, false, vec![], false).await;
    fixture.sources[1].mode.store(3, Ordering::Relaxed);
    let state = fixture.state.clone();
    let refresh = tokio::spawn(async move { refresh_subscriptions_foreground(&state).await });
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        fixture.sources[1].entered.notified(),
    )
    .await
    .unwrap();
    // Must complete while the older HTTP request is still blocked.
    tokio::time::timeout(std::time::Duration::from_secs(3), fixture.delete_a())
        .await
        .unwrap()
        .unwrap();
    fixture.sources[1].release.notify_one();
    assert!(matches!(
        refresh.await.unwrap(),
        Err(ConfigMutationError::Superseded)
    ));
    fixture.assert_removed(true).await;
}

#[tokio::test]
async fn subscription_delete_without_source_identity_fetches_instead_of_dropping_retained_nodes() {
    let fixture = Fixture::new(true, false, vec![], false).await;
    let path = &fixture.state.runtime_paths.sub_nodes_snapshot;
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("source_ids");
    legacy.as_object_mut().unwrap().remove("version");
    std::fs::write(path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    std::fs::remove_file(&fixture.state.runtime_paths.node_bindings).unwrap();
    *fixture.state.sub_nodes_cache.write().await = None;
    fixture.delete_a().await.unwrap();
    assert_eq!(fixture.sources[1].requests.load(Ordering::Relaxed), 2);
    fixture.assert_removed(true).await;
    assert!(fixture.runtime()["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["server"] == "b.example.com"));
}

#[tokio::test]
async fn subscription_delete_persistence_failure_restores_runtime_and_snapshot() {
    let fixture = Fixture::new(true, false, vec![], false).await;
    let before = std::fs::read(&fixture.state.runtime_paths.active_config).unwrap();
    let bindings = std::fs::read(&fixture.state.runtime_paths.node_bindings).unwrap();
    let snapshot = std::fs::read(&fixture.state.runtime_paths.sub_nodes_snapshot).unwrap();
    std::fs::remove_file(&fixture.state.config_path).unwrap();
    std::fs::create_dir(&fixture.state.config_path).unwrap();
    assert!(fixture.delete_a().await.is_err());
    assert_eq!(
        std::fs::read(&fixture.state.runtime_paths.active_config).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(&fixture.state.runtime_paths.node_bindings).unwrap(),
        bindings
    );
    assert_eq!(
        std::fs::read(&fixture.state.runtime_paths.sub_nodes_snapshot).unwrap(),
        snapshot
    );
    assert!(fixture
        .state
        .config
        .read()
        .await
        .subs
        .contains(&fixture.urls[0]));
    assert!(read_sub_nodes_snapshot(&fixture.state)
        .await
        .unwrap()
        .node_names
        .iter()
        .any(|name| name == "node-a"));
}

mod edge_cases;
