use std::sync::Arc;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::handlers::{
    nodes::{add_node, delete_node, get_nodes},
    proxy::set_last_proxy,
    service::{get_status, start_service, stop_service, test_connectivity},
    static_assets::{
        serve_bootstrap_css, serve_favicon, serve_font_woff, serve_font_woff2,
        serve_index, serve_vue,
    },
    subs::{add_sub, delete_sub, get_subs, refresh_subs},
    version::{get_version, upgrade},
};
use crate::state::AppState;

pub fn build_router(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/favicon.svg", get(serve_favicon))
        .route("/assets/vue.js", get(serve_vue))
        .route("/assets/bootstrap-icons.css", get(serve_bootstrap_css))
        .route("/assets/fonts/bootstrap-icons.woff2", get(serve_font_woff2))
        .route("/assets/fonts/bootstrap-icons.woff", get(serve_font_woff))
        .route("/api/status", get(get_status))
        .route("/api/service/start", post(start_service))
        .route("/api/service/stop", post(stop_service))
        .route("/api/connectivity", post(test_connectivity))
        .route("/api/version", get(get_version))
        .route("/api/upgrade", post(upgrade))
        .route("/api/subs", get(get_subs))
        .route("/api/subs", post(add_sub))
        .route("/api/subs", delete(delete_sub))
        .route("/api/subs/refresh", post(refresh_subs))
        .route("/api/nodes", get(get_nodes))
        .route("/api/nodes", post(add_node))
        .route("/api/nodes", delete(delete_node))
        .route("/api/last-proxy", post(set_last_proxy))
        .with_state(app_state)
}

#[cfg(test)]
mod tests {
    use axum::http::{header::CONTENT_TYPE, StatusCode};
    use serde_json::json;
    use tower::ServiceExt;

    use crate::{
        models::Config,
        test_support::{empty_request, json_request, response_json, response_text, test_app},
    };

    #[tokio::test]
    async fn router_serves_index_page() {
        let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(empty_request("GET", "/"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_text(response).await;
        assert!(body.contains("Miao 控制面板"));
    }

    #[tokio::test]
    async fn router_serves_favicon_with_svg_content_type() {
        let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(empty_request("GET", "/favicon.svg"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(CONTENT_TYPE).unwrap(), "image/svg+xml");
        let body = response_text(response).await;
        assert!(body.contains("<svg"));
    }

    #[tokio::test]
    async fn router_returns_status_payload() {
        let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(empty_request("GET", "/api/status"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["success"], true);
        assert_eq!(json["message"], "stopped");
        assert_eq!(json["data"]["running"], false);
    }

    #[tokio::test]
    async fn router_returns_node_list_payload() {
        let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"router-node","server":"node.example.com","server_port":443,"password":"secret","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"server_name":"sni.example.com","insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(empty_request("GET", "/api/nodes"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["success"], true);
        assert_eq!(json["message"], "Nodes loaded");
        assert_eq!(json["data"][0]["tag"], "router-node");
        assert_eq!(json["data"][0]["server"], "node.example.com");
        assert_eq!(json["data"][0]["sni"], "sni.example.com");
    }

    #[tokio::test]
    async fn router_returns_subscription_list_payload() {
        let app = test_app(Config {
            port: None,
            subs: vec!["https://example.com/subscription".to_string()],
            nodes: vec![],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(empty_request("GET", "/api/subs"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["success"], true);
        assert_eq!(json["message"], "Subscriptions loaded");
        assert_eq!(json["data"][0]["url"], "https://example.com/subscription");
        assert_eq!(json["data"][0]["node_count"], 0);
    }

    #[tokio::test]
    async fn router_rejects_duplicate_subscription_with_bad_request() {
        let app = test_app(Config {
            port: None,
            subs: vec!["https://example.com/subscription".to_string()],
            nodes: vec![],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(json_request(
                "POST",
                "/api/subs",
                json!({ "url": "https://example.com/subscription" }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(json["success"], false);
        assert_eq!(json["message"], "Subscription already exists");
    }

    #[tokio::test]
    async fn router_returns_not_found_when_deleting_missing_subscription() {
        let app = test_app(Config {
            port: None,
            subs: vec!["https://example.com/subscription".to_string()],
            nodes: vec![],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(json_request(
                "DELETE",
                "/api/subs",
                json!({ "url": "https://example.com/missing" }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_eq!(json["success"], false);
        assert_eq!(json["message"], "Subscription not found");
    }

    #[tokio::test]
    async fn router_rejects_duplicate_node_with_bad_request() {
        let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"router-node","server":"node.example.com","server_port":443,"password":"secret","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(json_request(
                "POST",
                "/api/nodes",
                json!({
                    "tag": "router-node",
                    "server": "node.example.com",
                    "server_port": 443,
                    "password": "secret"
                }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = response_json(response).await;
        assert_eq!(json["success"], false);
        assert_eq!(json["message"], "Node with this tag already exists");
    }

    #[tokio::test]
    async fn router_returns_not_found_when_deleting_missing_node() {
        let app = test_app(Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"router-node","server":"node.example.com","server_port":443,"password":"secret","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"insecure":true}}"#.to_string(),
            ],
            custom_rules: vec![],
        })
        .await;

        let response = app
            .oneshot(json_request(
                "DELETE",
                "/api/nodes",
                json!({ "tag": "missing-node" }),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json = response_json(response).await;
        assert_eq!(json["success"], false);
        assert_eq!(json["message"], "Node not found");
    }
}
