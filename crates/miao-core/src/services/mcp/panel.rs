use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Value as JsonValue};

use crate::models::VpsDeployRequest;
use crate::models::{
    BatchNodeRequest, DeleteNodeRequest, DeleteRuleRequest, MaxMultiplierRequest, McpRequest,
    NodeRequest, NodeSelectRequest, RouteMode, RouteModeRequest, RuleRequest,
    ScheduledRefreshRequest, SetNodeDisabledRequest, SubBatchRequest, SubRequest,
};
use crate::services::commands::{self, CommandReply, CommandResult};
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

fn command_payload<T: Serialize>(result: CommandResult<T>) -> Result<JsonValue, String> {
    let reply = result.map_err(|error| error.to_string())?;
    Ok(json!({ "message": reply.message, "data": reply.data }))
}

pub(super) fn response_data<T: Serialize>(response: CommandReply<T>) -> Result<JsonValue, String> {
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
    command_payload(commands::service::start_service(state.clone()).await)
}

pub(super) async fn stop_service(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "停止透明代理")?;
    command_payload(commands::service::stop_service(state.clone()).await)
}

pub(super) async fn list_subscriptions(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let response = commands::subs::get_subs(state.clone()).await;
    let subscriptions = response_data(response)?;
    Ok(json!({ "subscriptions": subscriptions }))
}

pub(super) async fn refresh_subscriptions(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let response = commands::subs::refresh_subs(state.clone())
        .await
        .map_err(|error| error.to_string())?;
    response_data(response)
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
    command_payload(commands::subs::add_subs_batch(state.clone(), request).await)
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
    command_payload(
        commands::subs::delete_sub(
            state.clone(),
            SubRequest {
                url: url.to_string(),
            },
        )
        .await,
    )
}

pub(super) async fn list_subscription_nodes(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let response = commands::subs::get_sub_nodes(state.clone()).await;
    let groups = response_data(response)?;
    Ok(json!({ "subscriptions": groups }))
}

pub(super) async fn set_subscription_node_disabled(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let request: SetNodeDisabledRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    command_payload(commands::subs::set_node_disabled(state.clone(), request).await)
}

pub(super) async fn scan_clash_verge(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let response = commands::subs::get_verge_import(state.clone()).await;
    response_data(response)
}

pub(super) async fn list_manual_nodes(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let response = commands::nodes::get_nodes(state.clone()).await;
    let nodes = response_data(response)?;
    Ok(json!({ "nodes": nodes }))
}

pub(super) async fn add_node(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    let request: NodeRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    command_payload(commands::nodes::add_node(state.clone(), request).await)
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
    command_payload(commands::nodes::import_nodes(state.clone(), request).await)
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
    command_payload(
        commands::nodes::delete_node(
            state.clone(),
            DeleteNodeRequest {
                tag: tag.to_string(),
            },
        )
        .await,
    )
}

pub(super) async fn set_route_mode(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let mode = args
        .get("mode")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `mode`".to_string())?;
    let route_mode = match mode {
        "rule" => RouteMode::Rule,
        "global" => RouteMode::Global,
        _ => return Err("Invalid params: `mode` 必须是 rule 或 global".to_string()),
    };
    let response =
        commands::service::set_route_mode(state.clone(), RouteModeRequest { route_mode })
            .await
            .map_err(|error| error.to_string())?;
    let changed = response.data.as_ref().is_some_and(|data| data.changed);
    let mut payload = response_data(response)?;
    payload["note"] = json!(if changed {
        "已写入易变层配置；OpenWrt/Linux 系统重启后回到 config.yaml 的启动默认值（未设置则规则分流）"
    } else {
        "未变化"
    });
    Ok(payload)
}

pub(super) async fn set_node_select(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let select = args
        .get("select")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `select`".to_string())?;
    let response = commands::service::set_node_select(
        state.clone(),
        NodeSelectRequest {
            node_select: select.to_string(),
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    let result = response
        .data
        .as_ref()
        .ok_or_else(|| "读取接口未返回预期数据".to_string())?;
    let note = if result.fell_back_to_manual {
        crate::services::config::REGION_FALLBACK
    } else if result.changed {
        "已保存节点选择偏好"
    } else {
        "未变化"
    };
    let mut payload = response_data(response)?;
    payload["note"] = json!(note);
    Ok(payload)
}

pub(super) async fn set_max_multiplier(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let request: MaxMultiplierRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    let response = commands::service::set_max_multiplier(state.clone(), request)
        .await
        .map_err(|error| error.to_string())?;
    let changed = response.data.as_ref().is_some_and(|data| data.changed);
    let mut payload = response_data(response)?;
    payload["note"] = json!(if changed {
        "已保存最高倍率偏好"
    } else {
        "未变化"
    });
    Ok(payload)
}

pub(super) async fn add_rule(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    let request: RuleRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    command_payload(commands::rules::add_rule(state.clone(), request).await)
}

pub(super) async fn delete_rule(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "删除自定义规则")?;
    let request: DeleteRuleRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    command_payload(commands::rules::delete_rule(state.clone(), request).await)
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

    let response = commands::service::test_connectivity(
        state.clone(),
        commands::service::ConnectivityRequest {
            url: url.to_string(),
        },
    )
    .await
    .data
    .ok_or_else(|| "读取接口未返回预期数据".to_string())?;
    Ok(json!({
        "url": url,
        "success": response.success,
        "latency_ms": response.latency_ms,
        "http_status": response.http_status,
        "error_kind": response.error_kind,
        "error": response.error,
        "note": "请求由 Miao 所在主机发出，不使用 HTTP_PROXY/HTTPS_PROXY 环境变量，但仍可能经过系统 TUN/路由。success 仅代表收到 HTTP 响应（含 4xx/5xx），不保证浏览器或指定代理节点可用。",
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
    let mut payload =
        command_payload(commands::settings::set_mcp(state.clone(), McpRequest { enabled }).await)?;
    if !enabled {
        payload["note"] = json!("MCP 已关闭；本次响应后 /mcp 将返回 404");
    }
    Ok(payload)
}

pub(super) async fn get_scheduled_refresh(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let response = commands::settings::get_scheduled_refresh(state.clone()).await;
    response_data(response)
}

pub(super) async fn set_scheduled_refresh(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let request: ScheduledRefreshRequest =
        serde_json::from_value(args.clone()).map_err(|err| format!("Invalid params: {err}"))?;
    command_payload(commands::settings::set_scheduled_refresh(state.clone(), request).await)
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

    {
        command_payload(
            commands::vps::deploy_vps(
                state.clone(),
                VpsDeployRequest {
                    ip: ip.to_string(),
                    password: password.to_string(),
                },
            )
            .await,
        )
    }
}

pub(super) async fn upgrade_miao(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    require_confirmation(args, "升级并重启 Miao")?;

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
}
