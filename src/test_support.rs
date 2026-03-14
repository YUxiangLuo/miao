use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header::CONTENT_TYPE, Request},
    Router,
};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::{
    models::Config,
    router::build_router,
    state::{AppState, SING_PROCESS, SUB_STATUS},
};

pub fn app_state(config: Config) -> Arc<AppState> {
    Arc::new(AppState {
        config: Mutex::new(config),
    })
}

pub async fn reset_test_globals() {
    *SING_PROCESS.lock().await = None;
    SUB_STATUS.lock().await.clear();
}

pub async fn test_app(config: Config) -> Router {
    reset_test_globals().await;
    build_router(app_state(config))
}

pub fn empty_request(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

pub fn json_request(method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub async fn response_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn response_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
