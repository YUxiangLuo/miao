use std::sync::Arc;

use axum::{
    routing::{any, delete, get, post},
    Router,
};

#[cfg(not(windows))]
use crate::handlers::version::upgrade;
#[cfg(not(windows))]
use crate::handlers::vps::deploy_vps;
use crate::handlers::{
    clash::{proxy_clash_http, proxy_clash_traffic},
    mcp::{handle_mcp, set_mcp},
    nodes::{add_node, delete_node, get_nodes, import_nodes},
    proxy::set_last_proxy,
    rules::{add_rule, delete_rule, get_rules},
    service::{
        get_status, set_node_select, set_route_mode, start_service, stop_service, test_connectivity,
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
        .route("/mcp", post(handle_mcp));

    #[cfg(not(windows))]
    let router = router
        .route("/api/upgrade", post(upgrade))
        .route("/api/vps/deploy", post(deploy_vps));

    router.with_state(app_state)
}

#[cfg(test)]
mod tests;
