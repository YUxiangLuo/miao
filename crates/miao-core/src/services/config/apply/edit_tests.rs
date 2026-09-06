use super::*;
use crate::{models::Region, test_support::isolated_stopped_state};

fn manual_config() -> Config {
    Config {
        nodes: vec![r#"{"type":"trojan","tag":"manual","server":"127.0.0.1","server_port":443,"password":"secret"}"#.into()],
        ..Config::default()
    }
}

#[tokio::test]
async fn abandoned_local_edit_has_no_side_effects_and_releases_lock() {
    let (_root, state) = isolated_stopped_state(Config::default());
    let mut edit = ConfigEdit::begin(&state).await;
    edit.candidate.route_mode = RouteMode::Global;
    assert!(state.config_update.try_lock().is_err());
    drop(edit);
    assert!(state.config_update.try_lock().is_ok());
    assert_eq!(*state.config.read().await, Config::default());
    assert!(!state.config_path.exists());
    assert!(!state.volatile_path.exists());
}

#[tokio::test]
async fn local_edit_cannot_turn_into_a_locked_subscription_fetch() {
    let (_root, state) = isolated_stopped_state(Config::default());
    let mut edit = ConfigEdit::begin(&state).await;
    edit.candidate
        .subs
        .push("http://127.0.0.1:1/must-not-fetch".into());
    let generation = state.sub_refresh_generation.load(Ordering::Relaxed);
    assert!(edit
        .commit()
        .await
        .unwrap_err()
        .to_string()
        .contains("edit_subscriptions"));
    assert_eq!(
        state.sub_refresh_generation.load(Ordering::Relaxed),
        generation
    );
    assert_eq!(*state.config.read().await, Config::default());
    assert!(!state.config_path.exists());
    assert!(state.config_update.try_lock().is_ok());
}

#[tokio::test]
async fn concurrent_local_mutation_reads_the_previous_commit_not_a_stale_clone() {
    let (_root, state) = isolated_stopped_state(Config::default());
    let mut edit = ConfigEdit::begin(&state).await;
    let rule = r#"{"domain_suffix":"example.com","action":"reject"}"#;
    edit.candidate.custom_rules.push(rule.into());
    let (started, waiting) = tokio::sync::oneshot::channel();
    let concurrent_state = state.clone();
    let concurrent = tokio::spawn(async move {
        started.send(()).unwrap();
        apply_route_mode(&concurrent_state, RouteMode::Global).await
    });
    waiting.await.unwrap();
    assert!(!concurrent.is_finished());
    edit.commit().await.unwrap();
    let (previous, update) = tokio::time::timeout(std::time::Duration::from_secs(2), concurrent)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(previous, RouteMode::Rule);
    assert_eq!(update, RuntimeUpdate::None);
    let config = state.config.read().await;
    assert_eq!(config.custom_rules, [rule]);
    assert_eq!(config.route_mode, RouteMode::Global);
    assert!(state.sing_process.lock().await.is_none());
}

#[tokio::test]
async fn unreadable_runtime_or_bindings_rejects_edit_before_installation() {
    for obstruct_runtime in [true, false] {
        let original = manual_config();
        let (_root, state) = isolated_stopped_state(original.clone());
        let (obstructed, untouched) = if obstruct_runtime {
            (
                &state.runtime_paths.active_config,
                &state.runtime_paths.node_bindings,
            )
        } else {
            (
                &state.runtime_paths.node_bindings,
                &state.runtime_paths.active_config,
            )
        };
        tokio::fs::create_dir_all(obstructed).await.unwrap();
        tokio::fs::write(untouched, b"original bytes")
            .await
            .unwrap();
        let mut edit = ConfigEdit::begin(&state).await;
        edit.candidate.route_mode = RouteMode::Global;
        assert!(edit.commit().await.is_err());
        assert!(obstructed.is_dir());
        assert_eq!(tokio::fs::read(untouched).await.unwrap(), b"original bytes");
        assert_eq!(*state.config.read().await, original);
        assert!(!state.config_path.exists());
        assert!(!state.runtime_paths.sub_nodes_snapshot.exists());
        assert!(state.sing_process.lock().await.is_none());
    }
}

#[tokio::test]
async fn both_preferences_restore_exact_bytes_or_absence_on_transaction_failure() {
    for selection in [true, false] {
        for existed in [true, false] {
            let original = manual_config();
            let (_root, state) = isolated_stopped_state(original.clone());
            // Fail the runtime transaction after the preference has been staged.
            // No kernel binary is present or required by this failure path.
            tokio::fs::create_dir_all(&state.runtime_paths.active_config)
                .await
                .unwrap();
            let path = if selection {
                &state.runtime_paths.node_select_preference
            } else {
                &state.runtime_paths.max_multiplier_preference
            };
            if existed {
                tokio::fs::write(path, b"exact prior bytes\n")
                    .await
                    .unwrap();
            }
            let result = if selection {
                apply_node_select(&state, NodeSelect::Fastest(Region::Jp))
                    .await
                    .map(|_| ())
            } else {
                apply_max_multiplier(&state, Some(NodeMultiplier::parse("2.5").unwrap()))
                    .await
                    .map(|_| ())
            };
            assert!(result.is_err());
            if existed {
                assert_eq!(tokio::fs::read(path).await.unwrap(), b"exact prior bytes\n");
            } else {
                assert!(!path.exists());
            }
            assert_eq!(
                *state.node_select_preference.read().await,
                NodeSelect::Manual
            );
            assert_eq!(*state.max_multiplier_preference.read().await, None);
            assert_eq!(*state.config.read().await, original);
            assert!(!state.config_path.exists());
            assert!(state.sing_process.lock().await.is_none());
            assert!(state.config_update.try_lock().is_ok());
        }
    }
}
