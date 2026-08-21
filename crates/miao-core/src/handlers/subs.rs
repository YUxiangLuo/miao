use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::{atomic::Ordering, Arc};

use crate::models::{
    ApiResponse, SubBatchRequest, SubBatchResult, SubRequest, SubStatus, SubscriptionState,
    VergeImportItem, VergeImportResult,
};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::{apply_config_change, regenerate_preserving_service_state};
use crate::services::verge;
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
                success: false,
                node_count: 0,
                state: SubscriptionState::Pending,
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
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

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
        Ok(_) => Ok(success_no_data("Subscription added")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

/// 扫描本机 clash-verge-rev 的订阅（只读）。未安装/无 remote 订阅时 found=false。
pub async fn get_verge_import(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<VergeImportResult>> {
    let subs = verge::scan().await.unwrap_or_default();
    let config = state.config.read().await;
    let items: Vec<VergeImportItem> = subs
        .into_iter()
        .map(|sub| VergeImportItem {
            already_added: config.subs.contains(&sub.url),
            name: sub.name,
            url: sub.url,
        })
        .collect();
    success(
        "Clash Verge subscriptions scanned",
        VergeImportResult {
            found: !items.is_empty(),
            items,
        },
    )
}

/// 批量添加订阅：与逐条调 add_sub 不同，全部 URL 在一次配置事务内提交，
/// 只触发一次生成/校验/热重载。已存在或批内重复的跳过并计数。
pub async fn add_subs_batch(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubBatchRequest>,
) -> HandlerResult<SubBatchResult> {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let mut urls: Vec<String> = Vec::with_capacity(req.urls.len());
    for raw in &req.urls {
        let url = raw.trim().to_string();
        if let Err(e) = Validator::subscription_url(&url) {
            return Err(status_error(StatusCode::BAD_REQUEST, e));
        }
        if !urls.contains(&url) {
            urls.push(url);
        }
    }

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();

    let mut skipped = 0usize;
    for url in urls {
        if new_config.subs.contains(&url) {
            skipped += 1;
            continue;
        }
        new_config.subs.push(url);
    }
    let added = new_config.subs.len() - old_config.subs.len();

    if added == 0 {
        return Ok(success(
            "No new subscriptions to add",
            SubBatchResult { added, skipped },
        ));
    }

    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success(
            "Subscriptions added",
            SubBatchResult { added, skipped },
        )),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn delete_sub(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SubRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

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
        Ok(_) => Ok(success_no_data("Subscription deleted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn refresh_subs(State(state): State<Arc<AppState>>) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let _config_update = state.config_update.lock().await;
    let config = state.config.read().await.clone();

    match regenerate_preserving_service_state(&config, &state).await {
        Ok(update) if update.updated() => Ok(success_no_data(
            "Subscriptions refreshed and runtime updated",
        )),
        Ok(_) => Ok(success_no_data("Subscriptions refreshed")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

#[cfg(test)]
mod tests {
    use axum::{extract::State, http::StatusCode, response::Json};

    use super::{add_subs_batch, get_subs, get_verge_import};
    use crate::{
        error::AppError,
        models::{Config, SubBatchRequest, SubscriptionState},
        test_support::app_state,
    };

    fn config_with_subs(subs: &[&str]) -> Config {
        Config {
            port: None,
            subs: subs.iter().map(|s| s.to_string()).collect(),
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
        }
    }

    // app_state 默认 initializing=true（写路径的 409 闸），handler 直测需手动关闸
    fn ready_state(config: Config) -> std::sync::Arc<crate::state::AppState> {
        let state = app_state(config);
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        state
    }

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
        let state = app_state(config_with_subs(&["https://example.com/sub"]));

        let Json(response) = get_subs(State(state)).await;

        assert!(response.success);
        assert_eq!(response.message, "Subscriptions loaded");
        let subs = response.data.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].url, "https://example.com/sub");
        assert!(!subs[0].success);
        assert_eq!(subs[0].state, SubscriptionState::Pending);
        assert_eq!(subs[0].node_count, 0);
        assert!(subs[0].error.is_none());
    }

    // 扫描依赖真实路径解析；开发与 CI 机器上都没有 clash-verge-rev → found=false。
    #[tokio::test]
    async fn verge_import_reports_not_found_without_verge_install() {
        let state = app_state(config_with_subs(&[]));

        let Json(response) = get_verge_import(State(state)).await;

        assert!(response.success);
        let result = response.data.unwrap();
        assert!(!result.found);
        assert!(result.items.is_empty());
    }

    #[tokio::test]
    async fn batch_rejects_invalid_urls_before_touching_config() {
        let state = ready_state(config_with_subs(&[]));

        let result = add_subs_batch(
            State(state),
            Json(SubBatchRequest {
                urls: vec!["not-a-url".to_string()],
            }),
        )
        .await;

        let Err((status, _)) = result else {
            panic!("invalid URL must be rejected")
        };
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // 全部重复时不进配置事务（不起内核、不落盘），直接计数返回。
    #[tokio::test]
    async fn batch_skips_existing_without_applying() {
        let state = ready_state(config_with_subs(&["https://example.com/sub"]));

        let result = add_subs_batch(
            State(state.clone()),
            Json(SubBatchRequest {
                urls: vec![
                    "https://example.com/sub".to_string(),
                    " https://example.com/sub ".to_string(), // 批内重复（先去重）
                ],
            }),
        )
        .await;

        let Ok(Json(response)) = result else {
            panic!("all-duplicate batch must succeed without applying")
        };
        assert!(response.success);
        let result = response.data.unwrap();
        assert_eq!(result.added, 0);
        assert_eq!(result.skipped, 1);
        assert_eq!(state.config.read().await.subs.len(), 1);
    }
}
