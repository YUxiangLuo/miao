use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::Arc;

use crate::models::{ApiResponse, SubRequest, SubStatus};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::{regenerate_and_restart, save_config};
use crate::state::AppState;
use crate::validation::Validator;

pub async fn get_subs(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<SubStatus>>> {
    let config = state.config.lock().await;
    let status_map = state.sub_status.lock().await;

    let subs_with_status: Vec<SubStatus> = config
        .subs
        .iter()
        .map(|url| {
            status_map.get(url).cloned().unwrap_or(SubStatus {
                url: url.clone(),
                success: true,
                node_count: 0,
                error: None,
            })
        })
        .collect();

    success("Subscriptions loaded", subs_with_status)
}

pub async fn add_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    if let Err(e) = Validator::subscription_url(&req.url) {
        return Err(status_error(StatusCode::BAD_REQUEST, e));
    }

    let config_clone;
    {
        let mut config = state.config.lock().await;

        if config.subs.contains(&req.url) {
            return Err(status_error(StatusCode::BAD_REQUEST, "Subscription already exists"));
        }

        config.subs.push(req.url);

        if let Err(e) = save_config(&config).await {
            return Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {}", e)));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone, &state).await {
        Ok(_) => Ok(success_no_data("Subscription added and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn delete_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    let config_clone;
    {
        let mut config = state.config.lock().await;

        let original_len = config.subs.len();
        config.subs.retain(|s| s != &req.url);

        if config.subs.len() == original_len {
            return Err(status_error(StatusCode::NOT_FOUND, "Subscription not found"));
        }

        if let Err(e) = save_config(&config).await {
            return Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save config: {}", e)));
        }
        config_clone = config.clone();
    }

    match regenerate_and_restart(&config_clone, &state).await {
        Ok(_) => Ok(success_no_data("Subscription deleted and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn refresh_subs(
    State(state): State<Arc<AppState>>,
) -> HandlerResult {
    let config = state.config.lock().await;
    let config_clone = config.clone();
    drop(config);

    match regenerate_and_restart(&config_clone, &state).await {
        Ok(_) => Ok(success_no_data("Subscriptions refreshed and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, response::Json};

    use super::get_subs;
    use crate::{
        models::Config,
        test_support::app_state,
    };

    #[tokio::test]
    async fn get_subs_returns_default_pending_status_when_status_missing() {
        let state = app_state(Config {
            port: None,
            subs: vec!["https://example.com/sub".to_string()],
            nodes: vec![],
            custom_rules: vec![],
        });

        let Json(response) = get_subs(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "Subscriptions loaded");
        let subs = response.data.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].url, "https://example.com/sub");
        assert!(subs[0].success);
        assert_eq!(subs[0].node_count, 0);
        assert!(subs[0].error.is_none());
    }
}
