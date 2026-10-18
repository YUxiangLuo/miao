use std::{sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{FromRequest, Request, State},
    http::{request::Parts, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post},
    Router,
};

use crate::handlers::version::upgrade;

use crate::handlers::vps::deploy_vps;
use crate::handlers::{
    clash::{proxy_clash_http, proxy_clash_traffic},
    mcp::{handle_mcp, handle_mcp_get, set_mcp},
    nodes::{add_node, delete_node, get_nodes, import_nodes},
    proxy::{set_last_proxy, switch_proxy},
    rules::{add_rule, delete_rule, get_rules},
    service::{
        get_scheduled_refresh, get_status, set_max_multiplier, set_node_select, set_route_mode,
        set_scheduled_refresh, start_service, stop_service, test_connectivity,
    },
    static_assets::{
        serve_favicon, serve_icon_192, serve_icon_512, serve_icon_maskable_512, serve_index,
        serve_manifest, serve_service_worker,
    },
    subs::{
        add_sub, add_subs_batch, delete_sub, get_sub_nodes, get_subs, get_verge_import,
        refresh_subs, set_node_disabled,
    },
    version::get_version,
};
use crate::state::AppState;

const REQUEST_BODY_TIMEOUT: Duration = Duration::from_secs(30);

pub fn build_router(app_state: Arc<AppState>) -> Router {
    let router = Router::new()
        .route("/", get(serve_index))
        .route("/favicon.svg", get(serve_favicon))
        .route("/manifest.webmanifest", get(serve_manifest))
        .route("/sw.js", get(serve_service_worker))
        .route("/icon-192.png", get(serve_icon_192))
        .route("/icon-512.png", get(serve_icon_512))
        .route("/icon-maskable-512.png", get(serve_icon_maskable_512))
        .route("/api/status", get(get_status))
        .route("/api/service/start", post(start_service))
        .route("/api/service/stop", post(stop_service))
        .route("/api/route-mode", post(set_route_mode))
        .route("/api/node-select", post(set_node_select))
        .route("/api/max-multiplier", post(set_max_multiplier))
        .route(
            "/api/scheduled-refresh",
            get(get_scheduled_refresh).post(set_scheduled_refresh),
        )
        .route("/api/connectivity", post(test_connectivity))
        .route("/api/clash/traffic", get(proxy_clash_traffic))
        .route("/api/clash/{*path}", any(proxy_clash_http))
        .route("/api/version", get(get_version))
        .route("/api/subs", get(get_subs))
        .route("/api/subs", post(add_sub))
        .route("/api/subs", delete(delete_sub))
        .route("/api/subs/refresh", post(refresh_subs))
        .route("/api/subs/nodes", get(get_sub_nodes))
        .route("/api/subs/nodes/disabled", post(set_node_disabled))
        .route("/api/subs/batch", post(add_subs_batch))
        .route("/api/import/clash-verge", get(get_verge_import))
        .route("/api/nodes", get(get_nodes))
        .route("/api/nodes", post(add_node))
        .route("/api/nodes/import", post(import_nodes))
        .route("/api/nodes", delete(delete_node))
        .route("/api/rules", get(get_rules))
        .route("/api/rules", post(add_rule))
        .route("/api/rules", delete(delete_rule))
        .route("/api/mcp", post(set_mcp))
        .route("/api/last-proxy", post(set_last_proxy))
        .route("/api/proxy/switch", post(switch_proxy))
        .route("/mcp", get(handle_mcp_get).post(handle_mcp));

    let router = router
        .route("/api/upgrade", post(upgrade))
        .route("/api/vps/deploy", post(deploy_vps));

    router
        .layer(middleware::from_fn_with_state(
            app_state.clone(),
            finish_mutations_after_disconnect,
        ))
        .with_state(app_state)
}

/// A reload can interrupt the very HTTP connection that requested it. Once a
/// complete write request is admitted, keep its transaction alive through
/// commit or rollback even if Hyper drops the response future. Receiving an
/// incomplete body, read-only polling and WebSocket streams remain cancellable.
async fn finish_mutations_after_disconnect(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    if request.method().is_safe() {
        return next.run(request).await;
    }

    // Buffer with Axum's usual body limit before detaching. A slow/incomplete
    // upload has not started a write and must not keep shutdown waiting. Parts
    // preserves headers/extensions (including route captures) for the handler.
    let (parts, body) = tokio::select! {
        biased;
        _ = state.lifecycle.wait_for_shutdown() => return shutting_down_response(&state),
        result = tokio::time::timeout(
            REQUEST_BODY_TIMEOUT,
            <(Parts, Bytes)>::from_request(request, &state),
        ) => match result {
            Ok(Ok(buffered)) => buffered,
            Ok(Err(response)) => return response,
            Err(_) => return crate::responses::status_error::<()>(
                StatusCode::REQUEST_TIMEOUT,
                "Request body timed out",
            ).into_response(),
        }
    };
    // Shutdown may have begun just as the last body chunk arrived. Admission
    // still checks the terminal flag under the same lock as shutdown.
    let Some(guard) = state.begin_mutation() else {
        return shutting_down_response(&state);
    };
    let request = Request::from_parts(parts, Body::from(body));

    // Dropping the JoinHandle detaches the task; it does not abort the accepted
    // operation. The guard stays in that task so shutdown waits for both REST
    // writes and MCP POST requests, including work outside config_update.
    match tokio::spawn(async move { guard.scope(next.run(request)).await }).await {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, "Mutation request task failed");
            crate::responses::status_error::<()>(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Mutation request task failed",
            )
            .into_response()
        }
    }
}

fn shutting_down_response(state: &AppState) -> Response {
    crate::responses::status_error::<()>(
        StatusCode::SERVICE_UNAVAILABLE,
        if state.lifecycle.is_shutting_down() {
            "Service is shutting down"
        } else {
            "Service is preparing an upgrade"
        },
    )
    .into_response()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cancellation_tests;
