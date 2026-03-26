use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::Arc;

use crate::models::{ApiResponse, SubRequest, SubStatus};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::{apply_config_change, regenerate_and_restart};
use crate::state::AppState;
use crate::validation::Validator;

pub async fn get_subs(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<SubStatus>>> {
    let config = state.config.read().await;
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

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();

    if new_config.subs.contains(&req.url) {
        return Err(status_error(
            StatusCode::BAD_REQUEST,
            "Subscription already exists",
        ));
    }

    new_config.subs.push(req.url);

    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success_no_data("Subscription added and sing-box restarted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn delete_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();

    let original_len = new_config.subs.len();
    new_config.subs.retain(|s| s != &req.url);

    if new_config.subs.len() == original_len {
        return Err(status_error(
            StatusCode::NOT_FOUND,
            "Subscription not found",
        ));
    }

    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success_no_data(
            "Subscription deleted and sing-box restarted",
        )),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn refresh_subs(State(state): State<Arc<AppState>>) -> HandlerResult {
    let _config_update = state.config_update.lock().await;
    let config = state.config.read().await;
    let config_clone = config.clone();
    drop(config);

    match regenerate_and_restart(&config_clone, &state).await {
        Ok(_) => Ok(success_no_data(
            "Subscriptions refreshed and sing-box restarted",
        )),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, response::Json};

    use super::get_subs;
    use crate::{error::AppError, models::Config, test_support::app_state};

    #[test]
    fn app_error_context_message_stays_user_visible() {
        let err = AppError::context(
            "Failed to apply config change; rolled back to previous config",
            AppError::message("new config invalid"),
        );

        assert_eq!(
            err.to_string(),
            "Failed to apply config change; rolled back to previous config: new config invalid"
        );
    }

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
