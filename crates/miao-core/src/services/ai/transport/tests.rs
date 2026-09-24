use super::*;
use axum::{
    body::to_bytes,
    http::{HeaderValue, Response as HttpResponse},
    routing::get,
};
use serde_json::json;

fn stored(responses: bool) -> Stored {
    Stored {
        revision: 1,
        enabled: true,
        port: 16162,
        proxy_port: 16163,
        outbound: "proxy".into(),
        internal_key: "internal-key-that-is-at-least-32-bytes".into(),
        access_hash: super::super::hash("access"),
        providers: vec![Provider {
            id: "provider".into(),
            name: "Provider".into(),
            base_url: "https://api.example.test/v1/".into(),
            api_key: "provider-secret".into(),
            responses,
            models: vec![crate::models::ai::AiModel {
                id: "alias".into(),
                upstream: "upstream/model".into(),
            }],
        }],
    }
}

#[test]
fn local_request_requires_exact_loopback_host_and_forbids_origin() {
    for host in ["localhost:16162", "127.0.0.1:16162"] {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_str(host).unwrap());
        assert!(local_request(&headers, 16162));
        headers.insert("origin", HeaderValue::from_static("https://example.test"));
        assert!(!local_request(&headers, 16162));
    }
    for host in [
        "example.test:16162",
        "localhost:16163",
        "user@localhost:16162",
        "[::1]:16162",
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_str(host).unwrap());
        assert!(!local_request(&headers, 16162), "accepted Host {host}");
    }
}

#[test]
fn selection_rewrites_only_top_level_model_and_preserves_request_json() {
    let body = json!({
        "model":"alias", "stream":true,
        "messages":[{"role":"user","content":"alias"}],
        "tools":[{"type":"function","function":{"name":"alias","parameters":{"const":"alias"}}}],
        "metadata":{"model":"alias"}
    });
    let (_, selected, url) =
        select_request(&stored(false), body.to_string().as_bytes(), false).unwrap();
    assert_eq!(selected["model"], "upstream/model");
    assert_eq!(selected["messages"], body["messages"]);
    assert_eq!(selected["tools"], body["tools"]);
    assert_eq!(selected["stream"], true);
    assert_eq!(selected["metadata"], body["metadata"]);
    assert_eq!(url, "https://api.example.test/v1/chat/completions");
}

#[test]
fn responses_endpoint_requires_provider_opt_in() {
    let bytes = br#"{"model":"alias","input":"hello"}"#;
    let error = select_request(&stored(false), bytes, true)
        .err()
        .expect("Responses API must require opt in");
    assert!(error.to_string().contains("Responses API"));
    let (_, _, url) = select_request(&stored(true), bytes, true).unwrap();
    assert_eq!(url, "https://api.example.test/v1/responses");
}

#[tokio::test]
async fn relay_preserves_sse_status_and_allowlisted_headers_only() {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().route(
        "/stream",
        get(|| async {
            HttpResponse::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("content-type", "text/event-stream")
                .header("retry-after", "3")
                .header("x-request-id", "request-1")
                .header("set-cookie", "secret=cookie")
                .header("x-internal", "secret")
                .body(Body::from("data: first\n\ndata: second\n\n"))
                .unwrap()
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .unwrap();
    let (_tx, cancel) = tokio::sync::watch::channel(0u64);
    let response = relay(upstream, cancel, Observation::new(None, Default::default()));
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(response.headers()["retry-after"], "3");
    assert_eq!(response.headers()["x-request-id"], "request-1");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(!response.headers().contains_key("set-cookie"));
    assert!(!response.headers().contains_key("x-internal"));
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(&bytes[..], b"data: first\n\ndata: second\n\n");
    server.abort();
}

#[tokio::test]
async fn relay_persists_completion_and_body_drop_once_without_content() {
    use crate::analytics::{ai::Filter, Analytics};
    let dir = tempfile::tempdir().unwrap();
    let store = Analytics::open(&dir.path().join("history.duckdb")).unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let data = "data: {\"choices\":[{\"delta\":{\"content\":\"never-store-this\"}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":23,\"completion_tokens\":5,\"total_tokens\":28}}\n\ndata: [DONE]\n\n";
    let app = Router::new().route(
        "/",
        get(move || async move { ([("content-type", "text/event-stream")], data) }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_tx, cancel) = tokio::sync::watch::channel(0u64);
    for consume in [true, false] {
        let upstream = reqwest::Client::new()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        let observation = Observation::new(
            Some(store.clone()),
            crate::models::ai::AiRequestRecord {
                model: "alias".into(),
                ..Default::default()
            },
        );
        let response = relay(upstream, cancel.clone(), observation);
        if consume {
            assert_eq!(to_bytes(response.into_body(), 4096).await.unwrap(), data);
        } else {
            drop(response);
        }
    }
    store.drain_ai().await;
    let result = store.ai_summary(Filter::default()).await.unwrap();
    assert_eq!(result.totals.requests, 2);
    assert_eq!(result.totals.succeeded, 1);
    assert_eq!(result.totals.cancelled, 1);
    assert_eq!(result.totals.usage_known, 1);
    assert_eq!(result.totals.input_tokens, 23);
    assert_eq!(result.totals.output_tokens, 5);
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("never-store-this"));
    server.abort();
}
