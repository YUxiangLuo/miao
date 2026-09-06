use super::*;
use crate::models::{Config, StableConfig};
use std::sync::Arc;

fn environment() -> (tempfile::TempDir, Environment) {
    let root = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(root.path()).unwrap();
    let cwd = base.join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    (
        root,
        Environment {
            default_config: ConfigPathResolution {
                path: base.join("config.yaml"),
                source: ConfigPathSource::EtcDefault,
            },
            runtime_root: base.join("runtime"),
            data_dir: base.join("data"),
            cwd,
            windows: false,
            preferences: PreferenceStore::Working,
        },
    )
}
fn resolve(options: RuntimeOptions, env: &Environment) -> ResolvedProfile {
    ResolvedProfile::resolve_in(&options, None, env).unwrap()
}
fn named(path: PathBuf, env: &Environment) -> ResolvedProfile {
    resolve(
        RuntimeOptions {
            config_path: Some(path),
            ..Default::default()
        },
        env,
    )
}

#[test]
fn default_paths_and_explicit_selection_of_default_are_compatible() {
    let (_root, mut env) = environment();
    for (windows, store) in [
        (false, PreferenceStore::Working),
        (false, PreferenceStore::Runtime),
        (true, PreferenceStore::Persistent),
    ] {
        env.windows = windows;
        env.preferences = store;
        let default = resolve(RuntimeOptions::default(), &env);
        let explicit = named(env.default_config.path.clone(), &env);
        assert_eq!(default.kind, ProfileKind::Default);
        assert_eq!(default.runtime, explicit.runtime);
        assert_eq!(default.volatile, explicit.volatile);
        assert_eq!(default.runtime.runtime_dir, env.runtime_root);
        let prefs = match store {
            PreferenceStore::Working => &env.cwd,
            PreferenceStore::Runtime => &env.runtime_root,
            PreferenceStore::Persistent => &env.data_dir,
        };
        assert_eq!(default.runtime.last_proxy, prefs.join(".last_proxy"));
        assert_eq!(
            default.runtime.node_select_preference,
            prefs.join(".node_select")
        );
        assert_eq!(
            default.runtime.max_multiplier_preference,
            prefs.join(".max_multiplier")
        );
        assert_eq!(
            default.volatile,
            if windows {
                &env.data_dir
            } else {
                &env.runtime_root
            }
            .join("volatile.yaml")
        );
        assert!(
            !env.runtime_root.exists(),
            "resolution must not create files"
        );
    }
}

#[test]
fn named_profiles_isolate_all_state_even_with_matching_stems() {
    let (_root, mut env) = environment();
    for (windows, store) in [
        (false, PreferenceStore::Working),
        (false, PreferenceStore::Runtime),
        (true, PreferenceStore::Persistent),
    ] {
        env.windows = windows;
        env.preferences = store;
        let a = named(env.cwd.join("travel.yaml"), &env);
        let b = named(env.cwd.join("travel.yml"), &env);
        let default = resolve(RuntimeOptions::default(), &env);
        for profile in [&a, &b] {
            assert_eq!(profile.kind, ProfileKind::Named);
            assert_ne!(profile.runtime.runtime_dir, default.runtime.runtime_dir);
            assert_ne!(profile.volatile, default.volatile);
            assert_ne!(
                profile.runtime.node_select_preference,
                default.runtime.node_select_preference
            );
            assert!(profile
                .runtime
                .runtime_dir
                .starts_with(env.runtime_root.join("profiles")));
            assert_eq!(
                profile
                    .runtime
                    .node_select_preference
                    .starts_with(&profile.runtime.runtime_dir),
                store == PreferenceStore::Runtime
            );
            assert_eq!(
                profile.volatile.starts_with(&profile.runtime.runtime_dir),
                !windows
            );
        }
        assert_ne!(a.runtime.active_config, b.runtime.active_config);
        assert_ne!(a.runtime.config_cache, b.runtime.config_cache);
        assert_ne!(a.runtime.sub_nodes_snapshot, b.runtime.sub_nodes_snapshot);
        assert_ne!(a.runtime.node_bindings, b.runtime.node_bindings);
        assert_ne!(a.runtime.last_proxy, b.runtime.last_proxy);
        assert_ne!(
            a.runtime.node_select_preference,
            b.runtime.node_select_preference
        );
        assert_ne!(
            a.runtime.max_multiplier_preference,
            b.runtime.max_multiplier_preference
        );
        assert_ne!(a.volatile, b.volatile);
    }
}

#[test]
fn named_profile_identity_survives_first_save_and_does_not_depend_on_cwd() {
    let (_root, mut env) = environment();
    let path = env.cwd.join("new/config.yaml");
    let before = named(path.clone(), &env);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{}").unwrap();
    env.cwd = env.data_dir.clone();
    let after = named(path, &env);
    assert_eq!(before.runtime, after.runtime);
    assert_eq!(before.volatile, after.volatile);
}

#[test]
fn runtime_and_volatile_overrides_have_explicit_precedence_on_all_platforms() {
    let (_root, mut env) = environment();
    for windows in [true, false] {
        env.windows = windows;
        env.preferences = if windows {
            PreferenceStore::Persistent
        } else {
            PreferenceStore::Working
        };
        let options = RuntimeOptions {
            config_path: Some("other.yaml".into()),
            runtime_dir: Some("isolated".into()),
            ..Default::default()
        };
        let profile = resolve(options.clone(), &env);
        assert_eq!(profile.runtime.runtime_dir, env.cwd.join("isolated"));
        assert_eq!(profile.volatile, env.cwd.join("isolated/volatile.yaml"));
        assert_eq!(
            profile.runtime.node_select_preference,
            env.cwd.join("isolated/.node_select")
        );
        let profile = resolve(
            RuntimeOptions {
                volatile_path: Some("override.yaml".into()),
                log_path: Some("custom.log".into()),
                ..options
            },
            &env,
        );
        assert_eq!(profile.volatile, env.cwd.join("override.yaml"));
        assert_eq!(profile.log, Some(env.cwd.join("custom.log")));
    }
}

#[cfg(unix)]
#[test]
fn symlink_and_relative_aliases_resolve_to_the_same_profile() {
    let (_root, env) = environment();
    let target = env.cwd.join("travel.yaml");
    std::fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, env.cwd.join("alias.yaml")).unwrap();
    let actual = named(target, &env);
    for path in ["./travel.yaml", "alias.yaml", "../cwd/travel.yaml"] {
        let alias = named(path.into(), &env);
        assert_eq!(alias.config.path, actual.config.path);
        assert_eq!(alias.runtime, actual.runtime);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn configuration_writes_preserve_the_symlink_and_update_its_resolved_target() {
    let (_root, env) = environment();
    let target = env.cwd.join("travel.yaml");
    let alias = env.cwd.join("alias.yaml");
    std::fs::write(&target, "{}").unwrap();
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    let profile = named(alias.clone(), &env);
    let state = Arc::new(
        crate::state::AppState::with_profile(StableConfig::default(), Config::default(), profile)
            .unwrap(),
    );
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
    crate::services::commands::settings::set_mcp(
        state,
        crate::models::McpRequest { enabled: true },
    )
    .await
    .unwrap();
    assert!(std::fs::symlink_metadata(&alias)
        .unwrap()
        .file_type()
        .is_symlink());
    let saved: StableConfig = yaml_serde::from_slice(&std::fs::read(&target).unwrap()).unwrap();
    assert!(saved.mcp);
}

#[cfg(unix)]
#[test]
fn non_utf8_profile_names_have_distinct_identities() {
    use std::os::unix::ffi::OsStringExt;
    let (_root, env) = environment();
    let a = named(
        env.cwd
            .join(std::ffi::OsString::from_vec(b"config-\xff.yaml".to_vec())),
        &env,
    );
    let b = named(
        env.cwd
            .join(std::ffi::OsString::from_vec(b"config-\xfe.yaml".to_vec())),
        &env,
    );
    assert_ne!(a.runtime, b.runtime);
}

#[tokio::test]
async fn legacy_bindings_are_copied_once_without_overwriting_new_identity() {
    let (_root, env) = environment();
    let profile = named(env.cwd.join("travel.yaml"), &env);
    let old = env.cwd.join("travel.node-bindings.json");
    tokio::fs::write(&old, b"original tag identity")
        .await
        .unwrap();
    profile.migrate_bindings().await.unwrap();
    assert_eq!(
        tokio::fs::read(&profile.runtime.node_bindings)
            .await
            .unwrap(),
        b"original tag identity"
    );
    tokio::fs::write(&profile.runtime.node_bindings, b"new identity")
        .await
        .unwrap();
    profile.migrate_bindings().await.unwrap();
    assert_eq!(
        tokio::fs::read(&profile.runtime.node_bindings)
            .await
            .unwrap(),
        b"new identity"
    );
    assert_eq!(
        tokio::fs::read(&old).await.unwrap(),
        b"original tag identity"
    );
    assert!(!profile.runtime.runtime_dir.exists());
}

#[test]
fn missing_paths_require_directory_ancestors_but_existing_files_are_valid() {
    let (_root, env) = environment();
    let file = env.cwd.join("existing.yaml");
    std::fs::write(&file, "{}").unwrap();
    assert_eq!(resolve_path(&file, &env.cwd).unwrap(), file);
    for suffix in ["child.yaml", "missing/child.yaml"] {
        assert!(
            resolve_path(&file.join(suffix), &env.cwd).is_err(),
            "{suffix}"
        );
    }
    let missing = env.cwd.join("new/nested/config.yaml");
    assert_eq!(resolve_path(&missing, &env.cwd).unwrap(), missing);
    assert!(
        !env.cwd.join("new").exists(),
        "resolution must not create directories"
    );
}

#[test]
fn an_unresolvable_unselected_default_does_not_block_an_explicit_profile() {
    let (_root, mut env) = environment();
    let obstruction = env.cwd.join("not-a-directory");
    std::fs::write(&obstruction, "x").unwrap();
    env.default_config.path = obstruction.join("config.yaml");
    assert!(ResolvedProfile::resolve_in(&RuntimeOptions::default(), None, &env).is_err());
    let explicit = named(env.cwd.join("independent.yaml"), &env);
    assert_eq!(explicit.kind, ProfileKind::Named);
}

#[tokio::test]
async fn named_profiles_do_not_restore_or_overwrite_another_profiles_preferences() {
    use serde_json::Value;
    use std::time::Duration;
    let (_root, env) = environment();
    let a_path = env.cwd.join("a.yaml");
    let b_path = env.cwd.join("b.yaml");
    let a = named(a_path.clone(), &env);
    let b = named(b_path.clone(), &env);
    let legacy = resolve(RuntimeOptions::default(), &env);
    for profile in [&legacy, &a] {
        tokio::fs::create_dir_all(profile.runtime.node_select_preference.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::create_dir_all(profile.volatile.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&profile.runtime.node_select_preference, "fastest_jp\n")
            .await
            .unwrap();
        tokio::fs::write(&profile.runtime.max_multiplier_preference, "2.5\n")
            .await
            .unwrap();
        tokio::fs::write(&profile.volatile, "route_mode: global\n")
            .await
            .unwrap();
    }
    tokio::fs::write(
        &legacy.runtime.config_cache,
        "must not be imported or cleared",
    )
    .await
    .unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (path, expected_select, expected_max, expected_route) in [
        (a_path.clone(), "fastest_jp", Some("2.5"), "global"),
        (b_path.clone(), "manual", None, "rule"),
        (a_path.clone(), "fastest_jp", Some("2.5"), "global"),
        (b_path.clone(), "manual", Some("6.5"), "rule"),
    ] {
        let profile = named(path.clone(), &env);
        let handle = crate::runtime::spawn_resolved(
            RuntimeOptions {
                bind_port: Some(0),
                skip_extract: true,
                ..Default::default()
            },
            profile,
        )
        .await
        .unwrap();
        let status = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status: Value = client
                    .get(format!("{}/api/status", handle.url()))
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                if status["data"]["initializing"] == false {
                    break status;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(status["data"]["requested_node_select"], expected_select);
        assert_eq!(status["data"]["max_multiplier"].as_str(), expected_max);
        assert_eq!(status["data"]["route_mode"], expected_route);
        assert_eq!(status["data"]["running"], false);
        if path == b_path {
            let reply: Value = client
                .post(format!("{}/api/max-multiplier", handle.url()))
                .json(&serde_json::json!({"max_multiplier":"6.5"}))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(reply["success"], true, "{reply}");
        }
        handle.shutdown().await;
    }
    assert_eq!(
        tokio::fs::read(&a.runtime.max_multiplier_preference)
            .await
            .unwrap(),
        b"2.5\n"
    );
    assert_eq!(
        tokio::fs::read(&b.runtime.max_multiplier_preference)
            .await
            .unwrap(),
        b"6.5\n"
    );
    assert_eq!(
        tokio::fs::read(&legacy.runtime.config_cache).await.unwrap(),
        b"must not be imported or cleared"
    );
    assert_eq!(
        tokio::fs::read(&legacy.runtime.max_multiplier_preference)
            .await
            .unwrap(),
        b"2.5\n"
    );
}

#[test]
fn temporary_directory_outlives_all_state_leases_but_unowned_directories_are_not_deleted() {
    let (_root, env) = environment();
    let prepared = crate::cli::prepare(crate::cli::LaunchSource::Subscription {
        url: "https://example.com/sub".into(),
        node_select: Default::default(),
    })
    .unwrap();
    let root = prepared.profile.as_ref().unwrap().path().to_path_buf();
    let profile = ResolvedProfile::resolve_in(&prepared.options, prepared.profile, &env).unwrap();
    let state = Arc::new(
        crate::state::AppState::with_profile(StableConfig::default(), Config::default(), profile)
            .unwrap(),
    );
    let worker = state.clone();
    drop(state);
    assert!(root.exists());
    drop(worker);
    assert!(!root.exists());
    let profile = named(env.cwd.join("unowned.yaml"), &env);
    drop(
        crate::state::AppState::with_profile(StableConfig::default(), Config::default(), profile)
            .unwrap(),
    );
    assert!(env.cwd.exists());
}
