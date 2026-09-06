use super::*;

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn default_help_and_version_are_supported() {
    assert_eq!(
        parse(args(&[])).unwrap(),
        Command::Run(LaunchSource::Default)
    );
    for flag in ["-h", "--help"] {
        assert_eq!(parse(args(&[flag])).unwrap(), Command::Help);
    }
    for flag in ["-V", "--version"] {
        assert_eq!(parse(args(&[flag])).unwrap(), Command::Version);
    }
}

#[test]
fn existing_config_syntax_is_preserved() {
    for values in [
        vec!["--config", "some path/config.yaml"],
        vec!["--config=some path/config.yaml"],
    ] {
        assert_eq!(
            parse(args(&values)).unwrap(),
            Command::Run(LaunchSource::Config(PathBuf::from("some path/config.yaml")))
        );
    }
    let prepared = prepare(LaunchSource::Config(PathBuf::from("profile.yaml"))).unwrap();
    assert!(prepared.options.config_path.unwrap().is_absolute());
    assert!(prepared._profile.is_none());
    assert!(prepared.options.runtime_dir.is_none());
}

#[cfg(unix)]
#[test]
fn separate_config_value_preserves_non_utf8_paths() {
    use std::os::unix::ffi::OsStringExt;
    let path = OsString::from_vec(b"/tmp/config-\xff.yaml".to_vec());
    assert_eq!(
        parse(vec![OsString::from("--config"), path.clone()]).unwrap(),
        Command::Run(LaunchSource::Config(PathBuf::from(path)))
    );
}

#[test]
fn subscription_region_is_case_insensitive_and_optional() {
    let url = "https://example.com/sub?a=1&token=test";
    for (code, region) in [
        ("hk", Region::Hk),
        ("JP", Region::Jp),
        ("tW", Region::Tw),
        ("Sg", Region::Sg),
        ("us", Region::Us),
    ] {
        for values in [
            vec!["--sub".to_string(), url.to_string(), code.to_string()],
            vec![format!("--sub={url}"), code.to_string()],
        ] {
            assert_eq!(
                parse(values.into_iter().map(OsString::from)).unwrap(),
                Command::Run(LaunchSource::Subscription {
                    url: url.to_string(),
                    node_select: NodeSelect::Fastest(region)
                })
            );
        }
    }
    assert_eq!(
        parse(args(&["--sub", url])).unwrap(),
        Command::Run(LaunchSource::Subscription {
            url: url.to_string(),
            node_select: NodeSelect::Manual
        })
    );
}

#[test]
fn subscription_and_config_are_mutually_exclusive_in_any_order() {
    for sub in [
        vec!["--sub", "https://example.com/sub"],
        vec!["--sub=https://example.com/sub"],
    ] {
        for config in [
            vec!["--config", "profile.yaml"],
            vec!["--config=profile.yaml"],
        ] {
            for values in [
                sub.iter().chain(&config).copied().collect::<Vec<_>>(),
                config.iter().chain(&sub).copied().collect(),
            ] {
                let error = parse(args(&values)).unwrap_err();
                assert!(error.to_string().contains("mutually exclusive"));
            }
        }
    }
}

#[test]
fn malformed_or_ambiguous_arguments_are_rejected() {
    for values in [
        vec!["--sub"],
        vec!["--sub="],
        vec!["--sub", ""],
        vec!["--sub", "--config", "profile.yaml"],
        vec!["--config"],
        vec!["--config="],
        vec!["--config", ""],
        vec!["--config", "--sub", "https://example.com"],
        vec![
            "--sub",
            "https://example.com",
            "--sub=https://other.example",
        ],
        vec!["--config=a", "--config", "b"],
        vec!["JP"],
        vec!["--config=a", "JP"],
        vec!["--sub", "https://example.com", "CN"],
        vec!["--sub", "https://example.com", "JP", "US"],
        vec!["--subs", "https://example.com"],
    ] {
        assert!(parse(args(&values)).is_err(), "must reject {values:?}");
    }
    for url in [
        "xxx",
        "file:///etc/passwd",
        "ftp://example.com",
        "https://",
        " ",
    ] {
        assert!(parse(args(&["--sub", url, "JP"])).is_err());
    }
}

#[test]
fn subscription_profiles_are_private_unique_and_cleaned_up() {
    let make = || {
        prepare(LaunchSource::Subscription {
            url: "https://example.com/sub".to_string(),
            node_select: NodeSelect::Fastest(Region::Jp),
        })
        .unwrap()
    };
    let first = make();
    let second = make();
    let root = first._profile.as_ref().unwrap().path().to_path_buf();
    assert_ne!(root, second._profile.as_ref().unwrap().path());
    for path in [
        &first.options.config_path,
        &first.options.volatile_path,
        &first.options.runtime_dir,
    ] {
        assert!(path.as_ref().unwrap().starts_with(&root));
    }
    let config: StableConfig = yaml_serde::from_str(
        &std::fs::read_to_string(first.options.config_path.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(config.subs, ["https://example.com/sub"]);
    assert_eq!(config.node_select, NodeSelect::Fastest(Region::Jp));
    assert!(config.max_multiplier.is_none());
    assert!(config.nodes.is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    drop(first);
    assert!(!root.exists());
    assert!(second._profile.as_ref().unwrap().path().exists());
}

#[cfg(unix)]
async fn exercise_subscription_startup(body: &'static str, expected_mode: &str) {
    use axum::{routing::get, Router};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_handler = calls.clone();
    let app = Router::new().route(
        "/sub",
        get(move || {
            calls_for_handler.fetch_add(1, Ordering::Relaxed);
            async move { body }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/sub", listener.local_addr().unwrap());
    let subscription_server =
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let Command::Run(source) = parse(args(&["--sub", &url, "JP"])).unwrap() else {
        panic!("run command")
    };
    let mut prepared = prepare(source).unwrap();
    let root = prepared._profile.as_ref().unwrap().path().to_path_buf();
    let runtime_dir = prepared.options.runtime_dir.as_ref().unwrap().clone();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    // Real initialization/HTTP/generation, but never a real kernel or TUN.
    let kernel = runtime_dir.join("sing-box");
    std::fs::write(&kernel, b"#!/bin/sh\nif [ \"$1\" = check ]; then exit 0; fi\nif [ \"$1\" = run ]; then trap ':' HUP; while :; do sleep 1; done; fi\nexit 1\n").unwrap();
    std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();
    prepared.options.bind_port = Some(0);
    prepared.options.skip_extract = true;
    prepared.options.open_browser = false;
    prepared.options.install_tracing = false;
    let handle = crate::spawn_server(prepared.options.clone()).await.unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let status = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status: serde_json::Value = client
                .get(format!("{}/api/status", handle.url()))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if status["data"]["ready"] == true
                && status["data"]["node_select"] == expected_mode
                && runtime_dir.join("config.json.cache").exists()
                && (expected_mode != "manual" || status["data"]["warning"].is_string())
            {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("CLI subscription should initialize without onboarding");
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(status["data"]["node_select"], expected_mode);
    assert_eq!(status["data"]["requested_node_select"], "fastest_jp");
    assert!(status["data"]["max_multiplier"].is_null());
    let active: serde_json::Value =
        serde_json::from_slice(&std::fs::read(runtime_dir.join("config.json")).unwrap()).unwrap();
    let proxy = active["outbounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["tag"] == "proxy")
        .unwrap();
    if expected_mode == "fastest_jp" {
        assert_eq!(proxy["type"], "urltest");
        assert_eq!(proxy["outbounds"], serde_json::json!(["日本-test"]));
        assert!(
            active["outbounds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["tag"] == "美国-test"),
            "filter candidates, not the complete node pool"
        );
    } else {
        assert_eq!(proxy["type"], "selector");
        assert!(status["data"]["warning"].as_str().unwrap().contains("手动"));
    }
    // Only a fallback needs to persist a different effective strategy.
    if expected_mode == "manual" {
        assert!(prepared.options.volatile_path.as_ref().unwrap().exists());
    }
    handle.shutdown().await;
    subscription_server.abort();
    drop(prepared);
    assert!(!root.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn sub_jp_starts_with_only_japanese_urltest_candidates() {
    exercise_subscription_startup("proxies:\n  - name: 日本-test\n    type: hysteria2\n    server: 127.0.0.1\n    port: 443\n    password: test\n  - name: 美国-test\n    type: hysteria2\n    server: 127.0.0.2\n    port: 443\n    password: test\n", "fastest_jp").await;
}

#[cfg(unix)]
#[tokio::test]
async fn sub_jp_without_japanese_nodes_keeps_requested_preference_on_fallback() {
    exercise_subscription_startup("proxies:\n  - name: 美国-test\n    type: hysteria2\n    server: 127.0.0.2\n    port: 443\n    password: test\n", "manual").await;
}

#[tokio::test]
async fn occupied_panel_port_rejects_sub_launch_before_kernel_extraction() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut prepared = prepare(LaunchSource::Subscription {
        url: "https://example.com/sub".to_string(),
        node_select: NodeSelect::Fastest(Region::Jp),
    })
    .unwrap();
    prepared.options.bind_port = Some(listener.local_addr().unwrap().port());
    prepared.options.open_browser = false;
    prepared.options.install_tracing = false;
    // This must return before initialization, even with extraction enabled.
    let result = crate::spawn_server(prepared.options.clone()).await;
    assert!(result.is_err());
    assert!(!prepared.options.runtime_dir.as_ref().unwrap().exists());
}
