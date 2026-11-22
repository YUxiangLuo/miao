use super::*;
use crate::models::{SetNodeDisabledRequest, SubscriptionRefreshPhase, SubscriptionState};
use crate::services::commands::subs::{get_subs, set_node_disabled};

impl Fixture {
    async fn set_b_disabled(&self, disabled: bool) {
        set_node_disabled(
            self.state.clone(),
            SetNodeDisabledRequest {
                sub: self.urls[1].clone(),
                name: "node-b".into(),
                disabled,
            },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn subscription_delete_settles_cancelled_rows_even_when_the_edit_is_rejected() {
    for (previous, reject_delete) in [
        (SubscriptionState::Ready, false),
        (SubscriptionState::Failed, false),
        (SubscriptionState::Pending, false),
        (SubscriptionState::Failed, true),
    ] {
        let manual = if reject_delete {
            vec![manual_node(Some("node-a"))]
        } else {
            vec![]
        };
        let fixture = Fixture::new(true, false, manual, false).await;
        if previous == SubscriptionState::Failed {
            fixture.sources[1].mode.store(1, Ordering::Relaxed);
            refresh_subscriptions_foreground(&fixture.state)
                .await
                .unwrap();
        } else if previous == SubscriptionState::Pending {
            // A local cache can exist before a new process has fetched a source.
            fixture
                .state
                .sub_status
                .lock()
                .await
                .remove(&fixture.urls[1]);
        }
        fixture.sources[1].mode.store(3, Ordering::Relaxed);
        let state = fixture.state.clone();
        let refresh = tokio::spawn(async move { refresh_subscriptions_foreground(&state).await });
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            fixture.sources[1].entered.notified(),
        )
        .await
        .unwrap();
        let before_delete = fixture.sources[1].requests.load(Ordering::Relaxed);
        if reject_delete {
            // Missing detours are now isolated, so force a real persistence
            // failure to exercise cancelled-row settlement on rollback.
            std::fs::remove_file(&fixture.state.config_path).unwrap();
            std::fs::create_dir(&fixture.state.config_path).unwrap();
        }
        let deletion = tokio::time::timeout(std::time::Duration::from_secs(3), fixture.delete_a())
            .await
            .unwrap();
        fixture.sources[1].release.notify_one();
        assert_eq!(deletion.is_err(), reject_delete);
        assert!(matches!(
            refresh.await.unwrap(),
            Err(ConfigMutationError::Superseded)
        ));
        let subs = get_subs(fixture.state.clone()).await.data.unwrap();
        let b = subs.iter().find(|sub| sub.url == fixture.urls[1]).unwrap();
        assert_eq!(b.state, previous);
        assert!(!subs
            .iter()
            .any(|sub| sub.state == SubscriptionState::Refreshing));
        assert_eq!(
            fixture.state.subscription_refresh.snapshot().phase,
            SubscriptionRefreshPhase::Idle
        );
        assert!(!fixture.state.subscription_refresh.foreground_in_flight());
        assert_eq!(
            fixture.sources[1].requests.load(Ordering::Relaxed),
            before_delete
        );
        assert_eq!(
            fixture
                .state
                .config
                .read()
                .await
                .subs
                .contains(&fixture.urls[0]),
            reject_delete
        );
    }
}

#[tokio::test]
async fn subscription_delete_disabled_cache_keeps_the_total_failure_guard() {
    for running in [false, true] {
        let fixture = Fixture::new(true, false, vec![], running).await;
        fixture.set_b_disabled(true).await;
        fixture.sources[1].mode.store(1, Ordering::Relaxed);
        assert!(
            refresh_subscriptions_foreground(&fixture.state)
                .await
                .unwrap()
                .fetch_succeeded
        );
        assert_eq!(
            fixture.state.sub_status.lock().await[&fixture.urls[1]].state,
            SubscriptionState::Failed
        );
        let paths = [
            &fixture.state.config_path,
            &fixture.state.runtime_paths.active_config,
            &fixture.state.runtime_paths.node_bindings,
            &fixture.state.runtime_paths.sub_nodes_snapshot,
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let pid = fixture
            .state
            .sing_process
            .lock()
            .await
            .as_ref()
            .and_then(|proc| proc.child.id());
        for _ in 0..2 {
            let requests = fixture.sources[1].requests.load(Ordering::Relaxed);
            assert!(fixture.delete_a().await.is_err());
            assert_eq!(
                fixture.sources[1].requests.load(Ordering::Relaxed),
                requests + 1
            );
            for (path, expected) in paths.iter().zip(&before) {
                assert_eq!(&std::fs::read(path).unwrap(), expected);
            }
            assert!(fixture
                .state
                .config
                .read()
                .await
                .subs
                .contains(&fixture.urls[0]));
            assert_eq!(
                fixture
                    .state
                    .sing_process
                    .lock()
                    .await
                    .as_ref()
                    .and_then(|proc| proc.child.id()),
                pid
            );
        }
        stop_sing_internal(&fixture.state).await;
    }
}

#[tokio::test]
async fn subscription_delete_does_not_accept_an_empty_pool_from_a_concurrent_local_edit() {
    let fixture = Fixture::new(true, false, vec![], false).await;
    // Pause deletion while it owns config_update and is reading the snapshot.
    // Queue a local edit so it commits after material selection but before the
    // deletion re-acquires the transaction lock. The local edit is legitimate:
    // A still keeps the node pool non-empty at that point.
    let snapshot_guard = fixture.state.sub_nodes_cache.write().await;
    let retired = fixture.state.sub_refresh_cancel.notified();
    tokio::pin!(retired);
    retired.as_mut().enable();
    let state = fixture.state.clone();
    let a = fixture.urls[0].clone();
    let deletion = tokio::spawn(async move { delete_sub(state, SubRequest { url: a }).await });
    tokio::time::timeout(std::time::Duration::from_secs(3), retired)
        .await
        .unwrap();
    let state = fixture.state.clone();
    let b = fixture.urls[1].clone();
    let (queued, waiting) = tokio::sync::oneshot::channel();
    let local_edit = tokio::spawn(async move {
        queued.send(()).unwrap();
        let mut edit = ConfigEdit::begin(&state).await;
        edit.candidate
            .disabled_nodes
            .push(crate::models::DisabledNode {
                sub: b,
                name: "node-b".into(),
            });
        edit.commit().await
    });
    waiting.await.unwrap();
    drop(snapshot_guard);
    tokio::time::timeout(std::time::Duration::from_secs(3), local_edit)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(3), deletion)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    let config = fixture.state.config.read().await;
    assert!(config.subs.contains(&fixture.urls[0]));
    assert!(config
        .disabled_nodes
        .iter()
        .any(|entry| entry.sub == fixture.urls[1]));
    assert!(fixture.runtime()["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["tag"] == "node-a"));
    assert_eq!(
        fixture.sources[1].requests.load(Ordering::Relaxed),
        1,
        "The cached result is not a fresh successful response"
    );
}

#[tokio::test]
async fn subscription_delete_empty_runtime_keeps_nodes_for_offline_reenable() {
    for running in [false, true] {
        let fixture = Fixture::new(true, false, vec![], running).await;
        fixture.set_b_disabled(true).await;
        fixture.delete_a().await.unwrap();
        fixture.assert_removed(false).await;
        let snapshot = read_sub_nodes_snapshot(&fixture.state).await.unwrap();
        assert_eq!(snapshot.node_names, ["node-b"]);
        let requests = fixture.sources[1].requests.load(Ordering::Relaxed);
        fixture.sources[1].mode.store(1, Ordering::Relaxed);
        fixture.set_b_disabled(false).await;
        fixture.assert_removed(true).await;
        assert_eq!(
            fixture.sources[1].requests.load(Ordering::Relaxed),
            requests,
            "Re-enable must use the retained local snapshot"
        );
        assert!(fixture.runtime()["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["tag"] == "node-b"));
        assert_eq!(fixture.state.sing_process.lock().await.is_some(), running);
        stop_sing_internal(&fixture.state).await;
    }
}

#[tokio::test]
async fn subscription_delete_empty_runtime_save_failure_restores_fingerprint_and_process() {
    for running in [false, true] {
        let fixture = Fixture::new(true, false, vec![], running).await;
        fixture.set_b_disabled(true).await;
        let paths = [
            &fixture.state.runtime_paths.active_config,
            &fixture.state.runtime_paths.config_cache,
            &fixture.state.runtime_paths.cache_manifest,
            &fixture.state.runtime_paths.node_bindings,
            &fixture.state.runtime_paths.sub_nodes_snapshot,
            &fixture.state.volatile_path,
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let pid = fixture
            .state
            .sing_process
            .lock()
            .await
            .as_ref()
            .and_then(|proc| proc.child.id());
        std::fs::remove_file(&fixture.state.config_path).unwrap();
        std::fs::create_dir(&fixture.state.config_path).unwrap();
        assert!(fixture.delete_a().await.is_err());
        for (path, expected) in paths.iter().zip(&before) {
            assert_eq!(&std::fs::read(path).unwrap(), expected);
        }
        assert!(fixture
            .state
            .config
            .read()
            .await
            .subs
            .contains(&fixture.urls[0]));
        assert_eq!(
            fixture
                .state
                .sing_process
                .lock()
                .await
                .as_ref()
                .and_then(|proc| proc.child.id()),
            pid
        );
        *fixture.state.sub_nodes_cache.write().await = None;
        let snapshot = read_sub_nodes_snapshot(&fixture.state).await.unwrap();
        assert!(snapshot.node_names.contains(&"node-a".to_string()));
        stop_sing_internal(&fixture.state).await;
    }
}

#[tokio::test]
async fn subscription_delete_empty_runtime_rejects_stale_disk_snapshot_after_write_failure() {
    let fixture = Fixture::new(true, false, vec![], false).await;
    fixture.set_b_disabled(true).await;
    let snapshot_path = &fixture.state.runtime_paths.sub_nodes_snapshot;
    let old_snapshot = std::fs::read(snapshot_path).unwrap();
    std::fs::remove_file(snapshot_path).unwrap();
    std::fs::create_dir(snapshot_path).unwrap();
    fixture.delete_a().await.unwrap();
    let accepted = read_sub_nodes_snapshot(&fixture.state).await.unwrap();
    assert_eq!(accepted.node_names, ["node-b"]);
    assert_eq!(accepted.subs, [fixture.urls[1].clone()]);
    assert!(!fixture.state.runtime_paths.active_config.exists());

    // A failed snapshot replacement may leave an old file behind. Its old
    // fingerprint must fail in a fresh AppState even though bindings survive.
    std::fs::remove_dir(snapshot_path).unwrap();
    std::fs::write(snapshot_path, old_snapshot).unwrap();
    let config = fixture.state.config.read().await.clone();
    let restarted = Arc::new(
        AppState::with_config_layers(
            StableConfig::from(&config),
            config,
            fixture.state.config_path.clone(),
            fixture.state.volatile_path.clone(),
            fixture.state.runtime_paths.clone(),
        )
        .unwrap(),
    );
    restarted.lifecycle.request_running(false);
    assert!(read_sub_nodes_snapshot(&restarted).await.is_none());

    // The live process still owns the accepted material despite that disk error.
    let requests = fixture.sources[1].requests.load(Ordering::Relaxed);
    fixture.sources[1].mode.store(1, Ordering::Relaxed);
    fixture.set_b_disabled(false).await;
    assert!(fixture.runtime()["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["tag"] == "node-b"));
    assert!(!fixture.runtime()["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|node| node["tag"] == "node-a"));
    assert_eq!(
        fixture.sources[1].requests.load(Ordering::Relaxed),
        requests
    );
}

#[tokio::test]
async fn subscription_delete_broken_manual_fallback_does_not_skip_fetching() {
    for running in [false, true] {
        for succeeds in [false, true] {
            let fixture =
                Fixture::new(true, false, vec![manual_node(Some("node-a"))], running).await;
            // A remains usable until deletion. Remove B from the accepted
            // snapshot; the raw manual count must not bypass fetching B.
            fixture.sources[1].mode.store(2, Ordering::Relaxed);
            refresh_subscriptions_foreground(&fixture.state)
                .await
                .unwrap();
            let before = fixture.sources[1].requests.load(Ordering::Relaxed);
            fixture.sources[1]
                .mode
                .store(if succeeds { 0 } else { 1 }, Ordering::Relaxed);
            let result = fixture.delete_a().await;
            assert_eq!(result.is_ok(), succeeds, "{result:?}");
            assert_eq!(
                fixture.sources[1].requests.load(Ordering::Relaxed),
                before + 1
            );
            assert_eq!(
                fixture
                    .state
                    .config
                    .read()
                    .await
                    .subs
                    .contains(&fixture.urls[0]),
                !succeeds
            );
            if succeeds {
                fixture.assert_removed(true).await;
            }
            stop_sing_internal(&fixture.state).await;
        }
    }
}

#[tokio::test]
async fn subscription_delete_empty_result_allows_new_slow_recovery() {
    for foreground in [false, true] {
        let fixture = Fixture::new(true, false, vec![], true).await;
        fixture.discard_snapshot().await;
        fixture.sources[1].mode.store(2, Ordering::Relaxed);
        fixture.delete_a().await.unwrap();
        assert!(!fixture.state.lifecycle.snapshot().ready);
        assert!(fixture.state.lifecycle.snapshot().should_run);
        let previous = fixture.state.sub_refresh_generation.load(Ordering::Relaxed);
        assert!(previous > 0);
        assert_eq!(
            fixture
                .state
                .sub_refresh_success_generation
                .load(Ordering::Relaxed),
            previous
        );
        assert_eq!(
            crate::runtime::recover_data_plane_once(&fixture.state).await,
            crate::runtime::RecoveryOutcome::WaitForNodes
        );
        assert!(fixture.state.sub_refresh_generation.load(Ordering::Relaxed) > previous);
        fixture.sources[1].mode.store(0, Ordering::Relaxed);
        let requests = fixture.sources[1].requests.load(Ordering::Relaxed);
        let state = fixture.state.clone();
        let recovery =
            tokio::spawn(async move { crate::runtime::retry_waiting_for_nodes(&state).await });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while fixture.state.subscription_refresh.snapshot().phase
                != SubscriptionRefreshPhase::Waiting
            {
                assert!(
                    !recovery.is_finished(),
                    "old successful deletion must not retire new recovery"
                );
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        if foreground {
            assert!(
                refresh_subscriptions_foreground(&fixture.state)
                    .await
                    .unwrap()
                    .fetch_succeeded
            );
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), recovery)
            .await
            .unwrap()
            .unwrap();
        let ready = fixture.state.lifecycle.snapshot().ready;
        let later_requests = fixture.sources[1].requests.load(Ordering::Relaxed);
        stop_sing_internal(&fixture.state).await;
        assert!(
            ready,
            "fresh nodes must activate without remote connectivity checks"
        );
        assert_eq!(
            later_requests,
            requests + 1,
            "a new foreground success must still cancel background recovery"
        );
    }
}

#[tokio::test]
async fn subscription_delete_cleanup_failure_preserves_config_files_and_process() {
    for running in [false, true] {
        for late_failure in [false, true] {
            // Root bypasses chmod; the non-file manifest still exercises
            // retirement failure and rollback in root-based test containers.
            if !late_failure && nix::unistd::geteuid().is_root() {
                continue;
            }
            let fixture = Fixture::new(true, false, vec![], running).await;
            fixture.discard_snapshot().await;
            fixture.sources[1].mode.store(2, Ordering::Relaxed);
            let paths = [
                &fixture.state.config_path,
                &fixture.state.runtime_paths.active_config,
                &fixture.state.runtime_paths.config_cache,
                &fixture.state.runtime_paths.node_bindings,
                &fixture.state.volatile_path,
            ];
            let before: Vec<_> = paths.iter().map(|path| std::fs::read(path).ok()).collect();
            let pid = fixture
                .state
                .sing_process
                .lock()
                .await
                .as_ref()
                .and_then(|process| process.child.id());
            let directory = &fixture.state.runtime_paths.runtime_dir;
            let permissions = std::fs::metadata(directory).unwrap().permissions();
            let manifest = &fixture.state.runtime_paths.cache_manifest;
            let manifest_bytes = std::fs::read(manifest).ok();
            if late_failure {
                if manifest_bytes.is_some() {
                    std::fs::remove_file(manifest).unwrap();
                }
                std::fs::create_dir(manifest).unwrap();
                std::fs::write(manifest.join("keep"), "unrelated contents").unwrap();
            } else {
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o555))
                    .unwrap();
            }
            let deletion = fixture.delete_a().await;
            std::fs::set_permissions(directory, permissions).unwrap();
            assert!(deletion.is_err(), "retirement failure must reject deletion");
            for (path, bytes) in paths.iter().zip(&before) {
                assert_eq!(&std::fs::read(path).ok(), bytes, "{}", path.display());
            }
            assert!(fixture
                .state
                .config
                .read()
                .await
                .subs
                .contains(&fixture.urls[0]));
            assert_eq!(
                fixture
                    .state
                    .sing_process
                    .lock()
                    .await
                    .as_ref()
                    .and_then(|process| process.child.id()),
                pid
            );
            if late_failure {
                assert_eq!(
                    std::fs::read_to_string(manifest.join("keep")).unwrap(),
                    "unrelated contents"
                );
                std::fs::remove_file(manifest.join("keep")).unwrap();
                std::fs::remove_dir(manifest).unwrap();
                if let Some(bytes) = manifest_bytes {
                    std::fs::write(manifest, bytes).unwrap();
                }
            }
            assert!(!std::fs::read_dir(directory).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".retired-runtime-")));
            // Once the filesystem is repaired, the same deletion succeeds and
            // later recovery can only start remaining-source nodes.
            fixture.delete_a().await.unwrap();
            fixture.assert_removed(false).await;
            fixture.sources[1].mode.store(0, Ordering::Relaxed);
            fixture.state.lifecycle.request_running(true);
            assert_eq!(
                crate::runtime::recover_data_plane_once(&fixture.state).await,
                crate::runtime::RecoveryOutcome::Complete
            );
            fixture.assert_removed(true).await;
            stop_sing_internal(&fixture.state).await;
        }
    }
}
