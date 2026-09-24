use super::*;
use crate::{
    models::{ai::AiProviderInput, Config},
    test_support::isolated_stopped_state,
};
use serde_json::json;

fn model(id: &str, upstream: &str) -> AiModel {
    AiModel {
        id: id.into(),
        upstream: upstream.into(),
    }
}

fn provider(id: &str, key: &str, models: Vec<AiModel>) -> Provider {
    Provider {
        id: id.into(),
        name: format!("{id} provider"),
        base_url: "https://api.example.test/v1".into(),
        api_key: key.into(),
        responses: false,
        models,
    }
}

fn valid_stored() -> Stored {
    Stored {
        revision: 4,
        enabled: false,
        port: 16162,
        proxy_port: 16163,
        outbound: "proxy".into(),
        providers: vec![provider(
            "one",
            "secret-provider-key",
            vec![model("public", "private")],
        )],
        internal_key: "internal-key-that-is-at-least-32-bytes".into(),
        access_hash: hash("client-secret"),
    }
}

fn input(stored: &Stored, api_key: Option<&str>) -> AiSettingsInput {
    AiSettingsInput {
        revision: stored.revision,
        enabled: false,
        port: stored.port,
        proxy_port: stored.proxy_port,
        outbound: stored.outbound.clone(),
        providers: stored
            .providers
            .iter()
            .map(|p| AiProviderInput {
                id: p.id.clone(),
                name: p.name.clone(),
                base_url: p.base_url.clone(),
                api_key: api_key.map(str::to_owned),
                responses: p.responses,
                models: p.models.clone(),
            })
            .collect(),
    }
}

#[test]
fn public_settings_projection_contains_no_provider_or_gateway_secrets() {
    let stored = valid_stored();
    let encoded = serde_json::to_value(stored.view()).unwrap();
    assert_eq!(encoded["providers"][0]["has_key"], true);
    assert_eq!(encoded["has_access_key"], true);
    let text = encoded.to_string();
    for secret in [
        &stored.providers[0].api_key,
        &stored.internal_key,
        "client-secret",
        &stored.access_hash,
    ] {
        assert!(!text.contains(secret), "public view leaked {secret}");
    }
    assert!(encoded["providers"][0].get("api_key").is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn initialize_creates_owner_only_secret_directory_and_admin_file() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, state) = isolated_stopped_state(Config::default());
    initialize(&state).await.unwrap();
    let dir = directory(&state);
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(dir.join("admin-token.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[tokio::test]
async fn disabled_save_preserves_none_key_but_rejects_explicit_empty_key_and_stale_revision() {
    let (_root, state) = isolated_stopped_state(Config::default());
    initialize(&state).await.unwrap();
    let stored = valid_stored();
    *state.ai.stored.write().await = stored.clone();

    let view = save(&state, input(&stored, None)).await.unwrap();
    assert_eq!(
        state.ai.stored.read().await.providers[0].api_key,
        "secret-provider-key"
    );

    let current = state.ai.stored.read().await.clone();
    assert!(save(&state, input(&current, Some(""))).await.is_err());
    assert_eq!(
        state.ai.stored.read().await.providers[0].api_key,
        "secret-provider-key"
    );

    let mut stale = input(&current, None);
    stale.revision = view.revision - 1;
    let error = save(&state, stale)
        .await
        .err()
        .expect("stale revision rejected");
    assert!(error.to_string().contains("重新加载"));
}

#[tokio::test]
async fn rotating_access_key_immediately_invalidates_the_previous_key() {
    let (_root, state) = isolated_stopped_state(Config::default());
    initialize(&state).await.unwrap();
    *state.ai.stored.write().await = valid_stored();
    let replacement = rotate_access_key(&state).await.unwrap().key;
    let stored = state.ai.stored.read().await;
    assert!(!matches_token(&stored.access_hash, "client-secret"));
    assert!(matches_token(&stored.access_hash, &replacement));
}

#[test]
fn validation_rejects_unsafe_urls_and_duplicate_public_aliases_across_providers() {
    for url in [
        "http://api.example.test/v1",
        "https://user:pass@api.example.test/v1",
        "https://api.example.test/v1?token=x",
    ] {
        let mut stored = valid_stored();
        stored.providers[0].base_url = url.into();
        assert!(
            validate_stored(&stored).is_err(),
            "accepted unsafe URL {url}"
        );
    }
    let mut stored = valid_stored();
    stored.providers.push(provider(
        "two",
        "another-key",
        vec![model("public", "other-private")],
    ));
    assert!(validate_stored(&stored)
        .unwrap_err()
        .to_string()
        .contains("唯一"));
}

#[tokio::test]
async fn injection_puts_ai_route_first_and_rejects_a_missing_selected_exit() {
    let gateway = Gateway::default();
    let mut stored = valid_stored();
    stored.enabled = true;
    stored.outbound = "removed-node".into();
    *gateway.stored.write().await = stored;
    let mut config = json!({
        "inbounds": [],
        "outbounds": [{"type":"selector","tag":"proxy","outbounds":["node"]},{"type":"direct","tag":"direct"}],
        "route": {"rules":[{"action":"sniff"},{"domain_suffix":["example.test"],"outbound":"direct"}]}
    });
    gateway.inject(&mut config).await.unwrap();
    assert_eq!(
        config["route"]["rules"][0],
        json!({"inbound":[INBOUND],"action":"reject"})
    );
    assert_eq!(config["route"]["rules"][1]["action"], "sniff");
    assert_eq!(
        config["outbounds"].as_array().unwrap().last().unwrap()["tag"],
        OUTBOUND
    );
    assert_eq!(
        config["outbounds"].as_array().unwrap().last().unwrap()["default"],
        "proxy"
    );
}

#[tokio::test]
async fn cache_matching_handles_disabled_legacy_and_detects_enabled_secret_or_port_drift() {
    let gateway = Gateway::default();
    assert!(gateway.cache_matches(b"{}").await);

    let mut stored = valid_stored();
    stored.enabled = true;
    *gateway.stored.write().await = stored.clone();
    let base = json!({"inbounds":[],"outbounds":[{"tag":"proxy"}],"route":{"rules":[]}});
    let mut injected = base.clone();
    gateway.inject(&mut injected).await.unwrap();
    assert!(
        gateway
            .cache_matches(&serde_json::to_vec(&injected).unwrap())
            .await
    );

    stored.internal_key.push_str("-rotated");
    *gateway.stored.write().await = stored.clone();
    assert!(
        !gateway
            .cache_matches(&serde_json::to_vec(&injected).unwrap())
            .await
    );
    stored.internal_key = valid_stored().internal_key;
    stored.proxy_port += 1;
    *gateway.stored.write().await = stored;
    assert!(
        !gateway
            .cache_matches(&serde_json::to_vec(&injected).unwrap())
            .await
    );
}

#[tokio::test]
async fn management_routes_require_admin_not_client_token_and_reject_cross_origin() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_root, state) = isolated_stopped_state(Config::default());
    initialize(&state).await.unwrap();
    let admin = tokio::fs::read_to_string(directory(&state).join("admin-token.txt"))
        .await
        .unwrap();
    let client = rotate_access_key(&state).await.unwrap().key;
    let store = crate::analytics::Analytics::open(&state.runtime_paths.analytics_db).unwrap();
    let _ = state.analytics.set(store);
    let app = crate::router::build_router(state);
    for path in ["/api/ai/settings", "/api/ai/analytics?days=7"] {
        for (token, origin, expected) in [
            ("", "http://localhost:6161", StatusCode::UNAUTHORIZED),
            (
                client.as_str(),
                "http://localhost:6161",
                StatusCode::UNAUTHORIZED,
            ),
            (
                admin.as_str(),
                "https://evil.test",
                StatusCode::UNAUTHORIZED,
            ),
            (admin.as_str(), "http://localhost:6161", StatusCode::OK),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("host", "localhost:6161")
                        .header("origin", origin)
                        .header("authorization", format!("Bearer {token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
    }
}

#[tokio::test]
async fn failed_persistence_does_not_publish_new_provider_or_revision() {
    let (_root, state) = isolated_stopped_state(Config::default());
    initialize(&state).await.unwrap();
    let old = valid_stored();
    *state.ai.stored.write().await = old.clone();
    // Atomic rename cannot replace a directory, including when tests run as root.
    std::fs::create_dir(directory(&state).join("settings.json")).unwrap();
    assert!(save(&state, input(&old, Some("replacement")))
        .await
        .is_err());
    assert_eq!(state.ai.stored.read().await.revision, old.revision);
    assert_eq!(
        state.ai.stored.read().await.providers[0].api_key,
        old.providers[0].api_key
    );
}

#[tokio::test]
async fn failed_kernel_apply_clears_staging_and_requires_recovery() {
    let (_root, state) = isolated_stopped_state(Config {
        nodes: vec![
            json!({"type":"shadowsocks", "tag":"fixture", "server":"127.0.0.1", "server_port":1234,"method":"aes-128-gcm","password":"synthetic"})
                .to_string(),
        ],
        ..Config::default()
    });
    initialize(&state).await.unwrap();
    let old = valid_stored();
    *state.ai.stored.write().await = old.clone();
    let mut candidate = input(&old, None);
    candidate.enabled = true;
    candidate.port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    // The isolated profile has no kernel executable: validation cannot apply.
    assert!(save(&state, candidate).await.is_err());
    assert!(!state.ai.stored.read().await.enabled);
    assert!(state.ai.error.read().await.is_some());
    let mut generated = json!({});
    state.ai.inject(&mut generated).await.unwrap();
    assert_eq!(generated, json!({}), "failed candidate remained staged");
}
