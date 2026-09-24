//! Full HTTP gateway against a TLS fixture and actual cropped kernel.
//! Namespace isolation is mandatory; this never starts the normal runtime/TUN.
use super::{fixture::*, *};
use crate::{
    models::{Config, RuntimePhase},
    state::{lifecycle::KernelOperation, SingBoxProcess},
    test_support::isolated_stopped_state,
};
use axum::http::StatusCode;
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

#[tokio::test]
#[ignore = "run scripts/test-ai-transport.sh; requires isolated Linux network namespace"]
async fn gateway_contract() {
    let host = std::env::var("MIAO_TEST_HOST_NETNS").expect("use scripts/test-ai-transport.sh");
    assert_ne!(
        std::fs::read_link("/proc/self/ns/net").unwrap(),
        PathBuf::from(host)
    );
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    let (root, state) = isolated_stopped_state(Config {
        nodes: vec![
            json!({"type":"shadowsocks","tag":"a","server":"127.0.0.1","server_port":1234,"method":"aes-128-gcm","password":"synthetic-a"}).to_string(),
            json!({"type":"shadowsocks","tag":"b","server":"127.0.0.1","server_port":1235,"method":"aes-128-gcm","password":"synthetic-b"}).to_string(),
        ],
        ..Config::default()
    });
    initialize(&state).await.unwrap();
    let store = crate::analytics::Analytics::open(&state.runtime_paths.analytics_db).unwrap();
    let _ = state.analytics.set(store.clone());
    let client_key = rotate_access_key(&state).await.unwrap().key;
    let seen = Observations::default();
    let (base, cert, provider_task) = provider(root.path(), seen.clone()).await;
    *state.ai.tls_root.lock().unwrap() = Some(cert);
    let api_port = reserve_port();
    let proxy_port = reserve_port();
    {
        let mut stored = state.ai.stored.write().await;
        stored.enabled = true;
        stored.port = api_port;
        stored.proxy_port = proxy_port;
        stored.outbound = "a".into();
        stored.providers = vec![Provider {
            id: "fixture".into(),
            name: "fixture".into(),
            base_url: format!("{base}/v1"),
            api_key: "synthetic-provider-key".into(),
            responses: true,
            models: vec![
                AiModel {
                    id: "alias".into(),
                    upstream: "upstream/model".into(),
                },
                AiModel {
                    id: "redirect".into(),
                    upstream: "redirect".into(),
                },
            ],
        }];
    }
    let mut config = json!({
        "log":{"level":"error"},
        "experimental":{"clash_api":{"external_controller":"127.0.0.1:6262"}},
        "dns":{"servers":[{"type":"local","tag":"local"}]},
        "inbounds":[],
        "outbounds":[{"type":"selector","tag":"proxy","outbounds":["a","b"],"default":"a","interrupt_exist_connections":false},
            {"type":"direct","tag":"direct"},
            {"type":"direct","tag":"a","inet4_bind_address":"127.0.0.2"},
            {"type":"direct","tag":"b","inet4_bind_address":"127.0.0.3"}],
        "route":{"default_domain_resolver":"local","rules":[{"action":"reject"}]}
    });
    state.ai.inject(&mut config).await.unwrap();
    let binary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../embedded/sing-box-amd64");
    let child = kernel(&binary, root.path(), "gateway", config.clone(), 6262).await;
    tokio::fs::write(
        &state.runtime_paths.active_config,
        serde_json::to_vec(&config).unwrap(),
    )
    .await
    .unwrap();
    *state.sing_process.lock().await = Some(SingBoxProcess {
        child,
        started_at: Instant::now(),
    });
    state.lifecycle.request_running(true);
    let generation = state.lifecycle.begin(KernelOperation::Start);
    state.lifecycle.finish(generation, RuntimePhase::Ready);
    start_listener(&state, bind(api_port).await.unwrap()).await;
    assert!(status(&state).await.ready);

    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let endpoint = format!("http://127.0.0.1:{api_port}/v1");
    let request = |path: &str, body: Value| {
        http.post(format!("{endpoint}/{path}"))
            .bearer_auth(&client_key)
            .header("cookie", "must-not-leak")
            .json(&body)
    };
    let body = json!({"model":"alias","messages":[{"role":"user","content":"synthetic"}],"tools":[{"type":"function","function":{"name":"tool"}}],"provider_extension":{"keep":true}});
    assert_eq!(
        http.get(format!("{endpoint}/models"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request("chat/completions", body.clone())
            .header("origin", "https://evil.test")
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let models: Value = http
        .get(format!("{endpoint}/models"))
        .bearer_auth(&client_key)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(models["data"][0]["id"], "alias");
    let first: Value = request("chat/completions", body.clone())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_exit(&first, "127.0.0.2");
    assert_eq!(first["body"]["model"], "upstream/model");
    assert_eq!(first["body"]["tools"], body["tools"]);
    assert_eq!(
        first["body"]["provider_extension"],
        body["provider_extension"]
    );
    assert_eq!(first["authorization"], "Bearer synthetic-provider-key");
    assert!(first["proxy_authorization"].is_null());
    assert!(first["cookie"].is_null());
    let mut stream = Events {
        response: request("chat/completions", json!({"model":"alias","stream":true}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap(),
        pending: vec![],
        expected: 0,
    };
    assert_exit(&stream.next().await, "127.0.0.2");
    // Warm an idle connection alongside SSE, then exercise the real save path.
    request("chat/completions", body.clone())
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let stored = state.ai.stored.read().await.clone();
    let mut input: AiSettingsInput =
        serde_json::from_value(serde_json::to_value(stored.view()).unwrap()).unwrap();
    input.outbound = "b".into();
    save(&state, input).await.unwrap();
    assert_eq!(
        state.lifecycle.snapshot().generation,
        generation,
        "exit switch restarted kernel"
    );
    let second: Value = request("responses", body.clone())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_exit(&second, "127.0.0.3");
    assert_exit(&stream.next().await, "127.0.0.2");
    let until = tokio::time::Instant::now() + Duration::from_secs(31);
    while tokio::time::Instant::now() < until {
        assert_exit(&stream.next().await, "127.0.0.2");
    }
    drop(stream);
    tokio::time::timeout(Duration::from_secs(3), async {
        while seen.streams.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("client disconnect must cancel upstream");
    assert_eq!(
        request("chat/completions", json!({"model":"redirect"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::TEMPORARY_REDIRECT
    );
    assert_eq!(
        request("chat/completions", json!({"model":"unknown"}))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    state.ai.stored.write().await.outbound = "removed".into();
    let count = seen.requests.load(Ordering::SeqCst);
    assert!(!status(&state).await.ready);
    assert_eq!(
        request("chat/completions", body.clone())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        seen.requests.load(Ordering::SeqCst),
        count,
        "missing exit silently fell back"
    );
    rotate_access_key(&state).await.unwrap();
    assert_eq!(
        request("chat/completions", body)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    store.drain_ai().await;
    let statistics = store
        .ai_summary(crate::analytics::ai::Filter::default())
        .await
        .unwrap();
    assert!(statistics.totals.succeeded >= 3);
    assert_eq!(statistics.totals.cancelled, 1);
    assert!(statistics
        .records
        .iter()
        .any(|r| r.outcome == "gateway_error"));
    assert!(statistics.records.iter().all(|r| r.model != "unknown"));
    assert!(statistics
        .records
        .iter()
        .any(|r| r.model == "alias" && r.upstream_model == "upstream/model"));
    shutdown(&state).await;
    let mut process = state.sing_process.lock().await.take().unwrap();
    process.child.kill().await.unwrap();
    provider_task.abort();
}
