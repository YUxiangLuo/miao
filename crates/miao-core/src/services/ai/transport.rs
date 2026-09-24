use std::{io, sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use serde_json::{json, Value};

use super::{matches_token, Pool, Provider, Stored, OUTBOUND};
use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

pub fn inference_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/models", get(models))
        .route("/v1/chat/completions", post(chat))
        .route("/v1/responses", post(responses))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state)
}

fn failure(status: StatusCode, message: &str) -> Response {
    (
        status,
        [("cache-control", "no-store")],
        Json(
            json!({"error":{"message":message,"type":"miao_gateway_error","code":status.as_u16()}}),
        ),
    )
        .into_response()
}

pub(crate) fn bearer(headers: &HeaderMap) -> &str {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("")
}

fn local_request(headers: &HeaderMap, port: u16) -> bool {
    if headers.contains_key("origin") {
        return false;
    }
    headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .and_then(|host| url::Url::parse(&format!("http://{host}")).ok())
        .is_some_and(|url| {
            matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
                && url.port_or_known_default() == Some(port)
                && url.username().is_empty()
                && url.password().is_none()
        })
}

async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Stored, (StatusCode, &'static str)> {
    let stored = state.ai.stored.read().await.clone();
    if !local_request(headers, stored.port) {
        return Err((
            StatusCode::FORBIDDEN,
            "Only local non-browser API clients are allowed",
        ));
    }
    if !matches_token(&stored.access_hash, bearer(headers)) {
        return Err((StatusCode::UNAUTHORIZED, "Invalid Miao API key"));
    }
    if !stored.enabled || state.lifecycle.is_shutting_down() {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "AI gateway is disabled"));
    }
    Ok(stored)
}

async fn models(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let stored = match authenticate(&state, &headers).await {
        Ok(value) => value,
        Err((status, message)) => return failure(status, message),
    };
    let data: Vec<_> = stored
        .providers
        .iter()
        .flat_map(|p| {
            p.models
                .iter()
                .map(|m| json!({"id":m.id,"object":"model","created":0,"owned_by":p.id}))
        })
        .collect();
    (
        [("cache-control", "no-store")],
        Json(json!({"object":"list","data":data})),
    )
        .into_response()
}

async fn chat(State(state): State<Arc<AppState>>, headers: HeaderMap, bytes: Bytes) -> Response {
    inference(state, headers, bytes, false).await
}

async fn responses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    inference(state, headers, bytes, true).await
}

fn select_request(
    stored: &Stored,
    bytes: &[u8],
    responses: bool,
) -> AppResult<(Provider, Value, String)> {
    let mut body: Value = serde_json::from_slice(bytes)
        .map_err(|_| AppError::message("Request must be a JSON object"))?;
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::message("A model is required"))?;
    let (provider, mapping) = stored
        .providers
        .iter()
        .find_map(|p| p.models.iter().find(|m| m.id == model).map(|m| (p, m)))
        .ok_or_else(|| AppError::message("Unknown model; list available models at /v1/models"))?;
    if responses && !provider.responses {
        return Err(AppError::message(
            "This provider has not enabled the Responses API",
        ));
    }
    if body.get("stream").is_some_and(|v| !v.is_boolean()) {
        return Err(AppError::message("stream must be a boolean"));
    }
    body["model"] = json!(mapping.upstream);
    let endpoint = if responses {
        "responses"
    } else {
        "chat/completions"
    };
    // base_url is the provider's API root, e.g. .../api/v1 or .../inference/v1.
    let url = format!("{}/{endpoint}", provider.base_url.trim_end_matches('/'));
    Ok((provider.clone(), body, url))
}

pub(super) async fn prepare_client(
    state: &AppState,
    stored: &Stored,
) -> AppResult<reqwest::Client> {
    if state.ai.error.read().await.is_some() {
        return Err(AppError::message("AI gateway needs configuration recovery"));
    }
    if !super::super::singbox::kernel_status(state).await.ready {
        return Err(AppError::message("Proxy kernel is not ready"));
    }
    let generation = state.lifecycle.snapshot().generation;
    let url = super::super::singbox::clash_api_url(&format!("/proxies/{OUTBOUND}"));
    let group: Value = state
        .http_client
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|_| AppError::message("AI proxy controller is unavailable"))?
        .error_for_status()
        .map_err(|_| AppError::message("AI proxy is not installed; save gateway settings again"))?
        .json()
        .await
        .map_err(|_| AppError::message("Invalid AI selector response"))?;
    if group["type"] != "Selector"
        || !group["all"]
            .as_array()
            .is_some_and(|all| all.contains(&json!(stored.outbound)))
    {
        return Err(AppError::message(
            "Selected AI exit is unavailable; choose another exit in gateway settings",
        ));
    }
    let changed = group["now"] != stored.outbound;
    if changed {
        state
            .http_client
            .put(&url)
            .timeout(Duration::from_secs(2))
            .json(&json!({"name":stored.outbound}))
            .send()
            .await
            .map_err(|_| AppError::message("Cannot select AI exit"))?
            .error_for_status()
            .map_err(|_| AppError::message("AI exit selection was rejected"))?;
    }
    let mut identity = stored.outbound.clone();
    if stored.outbound == "proxy" {
        let group: Value = state
            .http_client
            .get(super::super::singbox::clash_api_url("/proxies/proxy"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(|_| AppError::message("Global proxy is unavailable"))?
            .error_for_status()
            .map_err(|_| AppError::message("Global proxy is unavailable"))?
            .json()
            .await
            .map_err(|_| AppError::message("Invalid global proxy response"))?;
        identity.push_str(
            group["now"]
                .as_str()
                .ok_or_else(|| AppError::message("Global proxy has no selected exit"))?,
        );
    }
    let mut pool = state.ai.pool.lock().await;
    if changed
        || pool
            .as_ref()
            .is_none_or(|pool| pool.generation != generation || pool.outbound != identity)
    {
        *pool = Some(Pool {
            generation,
            outbound: identity,
            client: client(state, stored)?,
        });
    }
    Ok(pool.as_ref().unwrap().client.clone())
}

fn client(_state: &AppState, stored: &Stored) -> AppResult<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .no_proxy()
        .proxy(
            reqwest::Proxy::all(format!("http://127.0.0.1:{}", stored.proxy_port))?
                .basic_auth("miao", &stored.internal_key),
        )
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(15));
    #[cfg(test)]
    let builder = if let Some(cert) = _state.ai.tls_root.lock().unwrap().as_ref() {
        builder.add_root_certificate(cert.clone())
    } else {
        builder
    };
    builder
        .build()
        .map_err(|_| AppError::message("Unable to create AI HTTP client"))
}

async fn inference(
    state: Arc<AppState>,
    headers: HeaderMap,
    bytes: Bytes,
    responses: bool,
) -> Response {
    let (provider, body, url, client, mut cancel) = {
        // Serialize admission with gateway edits and kernel operations, not
        // with a provider's potentially multi-minute inference or SSE stream.
        let _guard = state.config_update.lock().await;
        let stored = match authenticate(&state, &headers).await {
            Ok(value) => value,
            Err((status, message)) => return failure(status, message),
        };
        let (provider, body, url) = match select_request(&stored, &bytes, responses) {
            Ok(value) => value,
            Err(err) => return failure(StatusCode::BAD_REQUEST, &err.to_string()),
        };
        let client = match prepare_client(&state, &stored).await {
            Ok(value) => value,
            Err(err) => return failure(StatusCode::SERVICE_UNAVAILABLE, &err.to_string()),
        };
        (provider, body, url, client, state.ai.cancel.subscribe())
    };
    // No incoming headers are forwarded: local access keys, cookies and
    // caller-provided upstream authentication must never reach the provider.
    let request = client.post(url).bearer_auth(&provider.api_key).json(&body);
    let response = tokio::select! {
        _ = cancel.changed() => return failure(StatusCode::SERVICE_UNAVAILABLE, "Gateway stopped"),
        result = tokio::time::timeout(Duration::from_secs(120), request.send()) => match result {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => return failure(StatusCode::BAD_GATEWAY, "Upstream connection failed; request was not retried"),
            Err(_) => return failure(StatusCode::GATEWAY_TIMEOUT, "Timed out waiting for upstream response; request was not retried"),
        },
    };
    relay(response, cancel)
}

fn relay(response: reqwest::Response, cancel: tokio::sync::watch::Receiver<u64>) -> Response {
    let status = response.status();
    let mut headers = HeaderMap::new();
    for name in ["content-type", "retry-after", "x-request-id"] {
        if let Some(value) = response.headers().get(name) {
            headers.insert(name, value.clone());
        }
    }
    headers.insert("cache-control", "no-store".parse().unwrap());
    let stream = futures::stream::try_unfold(
        (response.bytes_stream(), cancel),
        |(mut stream, mut cancel)| async move {
            tokio::select! {
                _ = cancel.changed() => Err(io::Error::new(io::ErrorKind::Interrupted, "AI gateway stopped")),
                chunk = tokio::time::timeout(Duration::from_secs(120), stream.next()) => match chunk {
                    Ok(Some(Ok(bytes))) => Ok(Some((bytes, (stream, cancel)))),
                    Ok(None) => Ok(None),
                    Ok(Some(Err(_))) => Err(io::Error::new(io::ErrorKind::ConnectionReset, "AI upstream stream interrupted")),
                    Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "AI upstream stream idle timeout")),
                },
            }
        },
    );
    (status, headers, Body::from_stream(stream)).into_response()
}

#[cfg(test)]
mod tests;
