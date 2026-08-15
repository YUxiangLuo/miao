use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;

use crate::models::{ApiResponse, MapSnapshot};
use crate::responses::success;
use crate::services::map::collect_map_snapshot;
use crate::state::AppState;

pub async fn get_map_snapshot(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<MapSnapshot>> {
    let snapshot = collect_map_snapshot(&state).await;
    success("Map snapshot loaded", snapshot)
}

#[cfg(test)]
mod tests {
    use crate::models::Config;
    use crate::test_support::{empty_request, response_json, test_app};
    use axum::http::StatusCode;
    use tower::ServiceExt;

    #[tokio::test]
    async fn map_snapshot_returns_client_and_manual_proxies_when_stopped() {
        let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"Tokyo 01","server":"tokyo.example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
        })
        .await;

        let response = app
            .oneshot(empty_request("GET", "/api/map/snapshot"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["success"], true);
        assert_eq!(json["data"]["client"]["type"], "client");
        assert_eq!(json["data"]["proxies"][0]["name"], "Tokyo 01");
        assert_eq!(json["data"]["proxies"][0]["server"], "tokyo.example.com");
        assert!(json["data"]["flows"].as_array().unwrap().is_empty());
    }
}
