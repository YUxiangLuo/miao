use std::sync::Arc;
use std::time::Instant;

use axum::{extract::State, response::Json};
use serde::Serialize;
use serde_json::{json, Value as JsonValue};
use tokio::time::Duration;

#[cfg(not(windows))]
use crate::models::VpsDeployRequest;
use crate::models::{
    BatchNodeRequest, DeleteNodeRequest, DeleteRuleRequest, McpRequest, NodeRequest, RuleRequest,
    SubBatchRequest, SubRequest,
};
use crate::responses::HandlerResult;
use crate::state::AppState;
use crate::validation::Validator;

fn require_confirmation(args: &JsonValue, action: &str) -> Result<(), String> {
    if args.get("confirm").and_then(JsonValue::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(format!(
            "执行{action}前必须先获得用户明确确认，然后传入 `confirm: true`"
        ))
    }
}

fn handler_payload<T: Serialize>(result: HandlerResult<T>) -> Result<JsonValue, String> {
    match result {
        Ok(Json(response)) => Ok(json!({
            "message": response.message,
            "data": response.data,
        })),
        Err((_status, Json(response))) => Err(response.message),
    }
}

fn response_data<T: Serialize>(
    response: crate::models::ApiResponse<T>,
) -> Result<JsonValue, String> {
    let data = response
        .data
        .ok_or_else(|| "读取接口未返回预期数据".to_string())?;
    serde_json::to_value(data).map_err(|err| format!("序列化响应失败: {err}"))
}

pub(super) async fn get_version_info(state: &Arc<AppState>) -> Result<JsonValue, String> {
    serde_json::to_value(crate::services::version::get_version_info(state).await)
        .map_err(|err| format!("序列化版本信息失败: {err}"))
}

pub(super) async fn start_service(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "启动透明代理")?;
    handler_payload(crate::handlers::service::start_service(State(state.clone())).await)
}

pub(super) async fn stop_service(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "停止透明代理")?;
    handler_payload(crate::handlers::service::stop_service(State(state.clone())).await)
}

pub(super) async fn list_subscriptions(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let Json(response) = crate::handlers::subs::get_subs(State(state.clone())).await;
    let subscriptions = response_data(response)?;
    Ok(json!({ "subscriptions": subscriptions }))
}

pub(super) async fn add_subscriptions(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let request: SubBatchRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    if request.urls.is_empty() {
        return Err("Invalid params: `urls` 不能为空".to_string());
    }
    handler_payload(
        crate::handlers::subs::add_subs_batch(State(state.clone()), Json(request)).await,
    )
}

pub(super) async fn delete_subscription(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "删除订阅")?;
    let url = args
        .get("url")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `url`".to_string())?;
    handler_payload(
        crate::handlers::subs::delete_sub(
            State(state.clone()),
            Json(SubRequest {
                url: url.to_string(),
            }),
        )
        .await,
    )
}

pub(super) async fn scan_clash_verge(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let Json(response) = crate::handlers::subs::get_verge_import(State(state.clone())).await;
    response_data(response)
}

pub(super) async fn list_manual_nodes(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let Json(response) = crate::handlers::nodes::get_nodes(State(state.clone())).await;
    let nodes = response_data(response)?;
    Ok(json!({ "nodes": nodes }))
}

pub(super) async fn add_node(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    let request: NodeRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    handler_payload(crate::handlers::nodes::add_node(State(state.clone()), Json(request)).await)
}

pub(super) async fn import_nodes(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let request: BatchNodeRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    if request.nodes.is_empty() {
        return Err("Invalid params: `nodes` 不能为空".to_string());
    }
    handler_payload(crate::handlers::nodes::import_nodes(State(state.clone()), Json(request)).await)
}

pub(super) async fn delete_node(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "删除手动节点")?;
    let tag = args
        .get("tag")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `tag`".to_string())?;
    handler_payload(
        crate::handlers::nodes::delete_node(
            State(state.clone()),
            Json(DeleteNodeRequest {
                tag: tag.to_string(),
            }),
        )
        .await,
    )
}

pub(super) async fn add_rule(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    let request: RuleRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    handler_payload(crate::handlers::rules::add_rule(State(state.clone()), Json(request)).await)
}

pub(super) async fn delete_rule(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "删除自定义规则")?;
    let request: DeleteRuleRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    handler_payload(crate::handlers::rules::delete_rule(State(state.clone()), Json(request)).await)
}

pub(super) async fn test_connectivity(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let url = args
        .get("url")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `url`".to_string())?;
    Validator::subscription_url(url).map_err(|err| format!("Invalid params: {err}"))?;

    let start = Instant::now();
    let response = state
        .http_client
        .head(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await;
    Ok(json!({
        "url": url,
        "success": response.is_ok(),
        "latency_ms": response.ok().map(|_| start.elapsed().as_millis() as u64),
        "note": "请求由 Miao 后端直连发出，不经过 HTTP_PROXY/HTTPS_PROXY 环境变量",
    }))
}

pub(super) async fn set_mcp_enabled(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "修改 MCP 端点状态")?;
    let enabled = args
        .get("enabled")
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| "Invalid params: missing `enabled`".to_string())?;
    let mut payload = handler_payload(
        crate::handlers::mcp::set_mcp(State(state.clone()), Json(McpRequest { enabled })).await,
    )?;
    if !enabled {
        payload["note"] = json!("MCP 已关闭；本次响应后 /mcp 将返回 404");
    }
    Ok(payload)
}

pub(super) async fn deploy_vps(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "部署远端 VPS")?;
    let ip = args
        .get("ip")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `ip`".to_string())?;
    let password = args
        .get("password")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `password`".to_string())?;

    #[cfg(not(windows))]
    {
        handler_payload(
            crate::handlers::vps::deploy_vps(
                State(state.clone()),
                Json(VpsDeployRequest {
                    ip: ip.to_string(),
                    password: password.to_string(),
                }),
            )
            .await,
        )
    }

    #[cfg(windows)]
    {
        let _ = (state, ip, password);
        Err("当前平台不支持 VPS 一键部署".to_string())
    }
}

pub(super) async fn upgrade_miao(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "升级并重启 Miao")?;

    #[cfg(not(windows))]
    {
        let result = crate::services::version::upgrade_binary(state)
            .await
            .map_err(|err| format!("升级失败: {err}"))?;
        Ok(if result == "Already up to date" {
            json!({ "upgraded": false, "message": result })
        } else {
            json!({
                "upgraded": true,
                "version": result,
                "note": "新版本已安装；Miao 将立即重启，本 MCP 端点会短暂断开",
            })
        })
    }

    #[cfg(windows)]
    {
        let _ = state;
        Err("Windows 不支持进程内升级，请下载安装包并退出 Miao 后覆盖安装".to_string())
    }
}
