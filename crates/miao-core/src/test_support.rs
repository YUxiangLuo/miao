use std::sync::Arc;

use axum::{
    body::{to_bytes, Body},
    http::{header::CONTENT_TYPE, Request},
    Router,
};
use serde_json::Value;

use crate::{
    models::{Config, StableConfig},
    paths::RuntimePaths,
    router::build_router,
    state::AppState,
};

/// Test thread names look like `foo::bar::baz`; `:` is illegal in Windows paths.
fn safe_test_path_component(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn app_state(config: Config) -> Arc<AppState> {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        safe_test_path_component(std::thread::current().name().unwrap_or("unnamed"))
    );
    let config_path = std::env::temp_dir().join(format!("miao-test-config-{unique}.yaml"));
    let volatile_path = std::env::temp_dir().join(format!("miao-test-volatile-{unique}.yaml"));
    let runtime_dir = std::env::temp_dir().join(format!("miao-test-runtime-{unique}"));
    Arc::new(
        AppState::with_config_layers(
            StableConfig::from(&config),
            config,
            config_path.clone(),
            volatile_path,
            RuntimePaths::new(runtime_dir, &config_path),
        )
        .expect("Failed to create AppState in test"),
    )
}

pub async fn reset_version_cache(state: &Arc<AppState>) {
    state
        .version_cache
        .store(Arc::new(crate::state::VersionCache {
            release: None,
            fetched_at: None,
        }));
}

pub async fn test_app(config: Config) -> Router {
    let state = app_state(config);
    state
        .initializing
        .store(false, std::sync::atomic::Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_path_component_strips_windows_illegal_chars() {
        assert_eq!(
            super::safe_test_path_component("services::config::tests::foo"),
            "services__config__tests__foo"
        );
        assert!(!super::safe_test_path_component("a:b").contains(':'));
    }
}
