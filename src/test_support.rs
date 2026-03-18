use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header::CONTENT_TYPE, Request},
    Router,
};
use serde_json::Value;

use crate::{
    models::Config,
    router::build_router,
    state::AppState,
};

pub fn app_state(config: Config) -> Arc<AppState> {
    Arc::new(AppState::new(config).expect("Failed to create AppState in test"))
}

pub async fn reset_version_cache(state: &Arc<AppState>) {
    let mut cache = state.version_cache.lock().await;
    cache.release = None;
    cache.fetched_at = None;
}

pub async fn test_app(config: Config) -> Router {
    let state = app_state(config);
    reset_version_cache(&state).await;
    build_router(state)
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
