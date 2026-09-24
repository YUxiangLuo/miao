use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::json;

use crate::{models::ai::AiSettingsInput, services::ai, state::AppState};

fn reply<T: Serialize>(value: T) -> Response {
    ([("cache-control", "no-store")], Json(value)).into_response()
}

fn error(status: StatusCode, message: impl ToString) -> Response {
    (
        status,
        [("cache-control", "no-store")],
        Json(json!({"error":message.to_string()})),
    )
        .into_response()
}

async fn authorized(state: &AppState, headers: &HeaderMap) -> bool {
    // The panel can be accessed remotely, but cross-origin browser requests
    // must never use the local administrator's credentials.
    if let Some(origin) = headers.get("origin") {
        let valid = origin
            .to_str()
            .ok()
            .and_then(|s| url::Url::parse(s).ok())
            .is_some_and(|url| {
                matches!(url.scheme(), "http" | "https")
                    && headers
                        .get("host")
                        .and_then(|h| h.to_str().ok())
                        .is_some_and(|host| {
                            url::Url::parse(&format!("{}://{host}", url.scheme()))
                                .ok()
                                .is_some_and(|expected| expected.origin() == url.origin())
                        })
            });
        if !valid {
            return false;
        }
    }
    if headers
        .get("sec-fetch-site")
        .is_some_and(|value| value == "cross-site")
    {
        return false;
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    ai::authorize(state, token).await
}

pub async fn status(State(state): State<Arc<AppState>>) -> Response {
    reply(ai::status(&state).await)
}

pub async fn settings(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers).await {
        return error(StatusCode::UNAUTHORIZED, "管理令牌无效或请求来源不受信任");
    }
    reply(ai::settings(&state).await)
}

pub async fn save(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(input): Json<AiSettingsInput>,
) -> Response {
    if !authorized(&state, &headers).await {
        return error(StatusCode::UNAUTHORIZED, "管理令牌无效或请求来源不受信任");
    }
    // Dropping an HTTP connection must not cancel a partially applied edit.
    match tokio::spawn(async move { ai::save(&state, input).await }).await {
        Ok(Ok(value)) => reply(value),
        Ok(Err(err)) => error(StatusCode::BAD_REQUEST, err),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "AI 配置任务失败"),
    }
}

pub async fn rotate(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers).await {
        return error(StatusCode::UNAUTHORIZED, "管理令牌无效或请求来源不受信任");
    }
    match tokio::spawn(async move { ai::rotate_access_key(&state).await }).await {
        Ok(Ok(value)) => reply(value),
        Ok(Err(err)) => error(StatusCode::BAD_REQUEST, err),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "AI 密钥任务失败"),
    }
}
