//! MCP（Model Context Protocol）端点核心：无状态 JSON-RPC 2.0 over POST /mcp。
//! 协议版本 2026-07-28：无握手、无会话，每个请求自描述。
//! 节点模型与面板一致：全部订阅 + 手动节点构成一个平铺节点池，没有分组概念；
//! 运行时的 sing-box selector（tag "proxy"）只是实现细节，不向 MCP 暴露。

mod catalog;
mod panel;

use catalog::tools_catalog;
use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde_json::{json, Value as JsonValue};

use crate::error::AppResult;
use crate::models::{LastProxy, NodeSelect, RouteMode};
use crate::services::{
    config::{RuntimeUpdate, REGION_FALLBACK},
    singbox::{is_sing_box_running, kernel_status, CLASH_API_BASE, CLASH_TRAFFIC_WS},
    status::{legacy_warning, runtime_warnings},
};
use crate::state::AppState;
use crate::VERSION;

pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
/// 配置模板里唯一的 selector；全部节点的容器，对外不暴露
const SELECTOR_TAG: &str = "proxy";
const DELAY_TIMEOUT_MS: u64 = 3000;
/// 与 builder.rs 生成的 urltest 组测速目标一致；sing-box Clash API 会拒绝
/// http:// 测速 URL 并回退到此 https 默认值，显式传它保证口径统一。
const DELAY_TEST_URL: &str = "https://www.gstatic.com/generate_204";
const DELAY_CONCURRENCY: usize = 6;
/// 测速请求的 HTTP 层超时：探测本身 3s，留 2s 余量兜底 sing-box 卡顿
const DELAY_HTTP_TIMEOUT: Duration = Duration::from_millis(DELAY_TIMEOUT_MS + 2000);

/// 处理一个 JSON-RPC 请求体。通知（无 id）返回 None，由 HTTP 层回 202。
pub async fn handle(state: &Arc<AppState>, body: &[u8]) -> Option<JsonValue> {
    let request: JsonValue = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(err) => {
            return Some(rpc_error(
                JsonValue::Null,
                -32700,
                &format!("Parse error: {err}"),
            ))
        }
    };

    let id = request.get("id").cloned().unwrap_or(JsonValue::Null);
    let method = request.get("method").and_then(JsonValue::as_str);

    // 通知没有 id：不处理、不应答（MCP 客户端启动后会发 notifications/initialized 等）
    let _ = request.get("id")?;

    let Some(method) = method else {
        return Some(rpc_error(id, -32600, "Invalid Request: missing method"));
    };

    let result = match method {
        "initialize" | "server/discover" => Ok(discover_result()),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": tools_catalog(),
            // 工具目录是静态的，客户端可长期缓存
            "ttlMs": 86_400_000u64,
        })),
        "tools/call" => match handle_tool_call(state, request.get("params")).await {
            Ok(payload) => Ok(tool_result(payload)),
            Err(message) => Ok(tool_error_result(&message)),
        },
        _ => Err((-32601, format!("Method not found: {method}"))),
    };

    Some(match result {
        Ok(value) => rpc_result(id, value),
        Err((code, message)) => rpc_error(id, code, &message),
    })
}

fn discover_result() -> JsonValue {
    json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "miao", "version": VERSION },
        // 给调用者的使用说明：客户端连接时读取。核心目的——防「自伤」：
        // agent 的出网流量很可能正经过本代理，破坏性操作会断它自己的网
        "instructions": "Miao 是本机/路由器的透明代理控制面，你的出网流量很可能正经过它。读取状态、列表和版本没有配置副作用；切换当前节点不重启内核，通常只影响新连接。修改订阅/节点/规则/模式会校验并热应用配置，可能短暂影响连接。停止服务、删除配置、部署 VPS、关闭 MCP 或升级 Miao 属于破坏性操作：执行前必须向用户说明具体影响并取得明确确认，绝不能自行把 confirm 设为 true。订阅 URL、连接记录和 VPS 密码属于敏感信息，不要在回答中无必要地复述。",
    })
}

fn rpc_result(id: JsonValue, result: JsonValue) -> JsonValue {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: JsonValue, code: i64, message: &str) -> JsonValue {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}

/// 工具成功结果：payload 以 JSON 文本装进 content（MCP 惯例）
fn tool_result(payload: JsonValue) -> JsonValue {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string()),
        }],
    })
}

/// 工具级失败：不是协议错误，isError 让客户端把信息带给模型
fn tool_error_result(message: &str) -> JsonValue {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

async fn handle_tool_call(
    state: &Arc<AppState>,
    params: Option<&JsonValue>,
) -> Result<JsonValue, String> {
    let params = params.cloned().unwrap_or(JsonValue::Null);
    let name = params
        .get("name")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing tool name".to_string())?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "get_status" => tool_get_status(state).await,
        "get_version_info" => panel::get_version_info(state).await,
        "start_service" => panel::start_service(state, &args).await,
        "stop_service" => panel::stop_service(state, &args).await,
        "list_subscriptions" => panel::list_subscriptions(state).await,
        "add_subscriptions" => panel::add_subscriptions(state, &args).await,
        "delete_subscription" => panel::delete_subscription(state, &args).await,
        "refresh_subscriptions" => tool_refresh_subscriptions(state).await,
        "scan_clash_verge" => panel::scan_clash_verge(state).await,
        "list_subscription_nodes" => panel::list_subscription_nodes(state).await,
        "set_subscription_node_disabled" => {
            panel::set_subscription_node_disabled(state, &args).await
        }
        "list_nodes" => tool_list_nodes(state).await,
        "list_manual_nodes" => panel::list_manual_nodes(state).await,
        "add_node" => panel::add_node(state, &args).await,
        "import_nodes" => panel::import_nodes(state, &args).await,
        "delete_node" => panel::delete_node(state, &args).await,
        "switch_node" => tool_switch_node(state, &args).await,
        "set_node_select" => tool_set_node_select(state, &args).await,
        "test_delay" => tool_test_delay(state, &args).await,
        "set_route_mode" => tool_set_route_mode(state, &args).await,
        "list_rules" => tool_list_rules(state).await,
        "add_rule" => panel::add_rule(state, &args).await,
        "delete_rule" => panel::delete_rule(state, &args).await,
        "get_traffic" => tool_get_traffic(state).await,
        "list_connections" => tool_list_connections(state, &args).await,
        "test_connectivity" => panel::test_connectivity(state, &args).await,
        "set_mcp_enabled" => panel::set_mcp_enabled(state, &args).await,
        "deploy_vps" => panel::deploy_vps(state, &args).await,
        "upgrade_miao" => panel::upgrade_miao(state, &args).await,
        other => Err(format!("Unknown tool: {other}")),
    }
}

// ── Clash API 小助手（面板控制面也走它，127.0.0.1:6262）──────────────────

async fn clash_get(state: &Arc<AppState>, path: &str) -> AppResult<JsonValue> {
    let url = format!("{CLASH_API_BASE}{path}");
    let response = state
        .http_client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await?;
    Ok(response.json::<JsonValue>().await?)
}

/// 拉 selector 全量信息（all + now）；服务未运行时 Clash API 不可达，返回 Err
async fn fetch_proxies(state: &Arc<AppState>) -> AppResult<JsonValue> {
    clash_get(state, "/proxies").await
}

fn selector_view(proxies: &JsonValue) -> Option<(Vec<String>, Option<String>)> {
    let selector = proxies.get(SELECTOR_TAG)?;
    let all = selector
        .get("all")?
        .as_array()?
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect();
    let now = selector
        .get("now")
        .and_then(JsonValue::as_str)
        .map(str::to_string);
    Some((all, now))
}

/// 平铺节点池：全部运行时 outbound，排除内置 proxy/direct 与分组项。
/// 与 generate::known_rule_targets 同口径——不随 node_select 的地区过滤收缩
fn flat_node_pool(proxies: &JsonValue) -> Vec<String> {
    let Some(map) = proxies.as_object() else {
        return Vec::new();
    };
    let mut names: Vec<String> = map
        .iter()
        .filter(|(name, node)| {
            name.as_str() != SELECTOR_TAG
                && name.as_str() != "direct"
                && !matches!(
                    node.get("type").and_then(JsonValue::as_str),
                    Some("Selector") | Some("URLTest")
                )
        })
        .map(|(name, _)| name.clone())
        .collect();
    names.sort();
    names
}

// ── 工具实现 ─────────────────────────────────────────────────────────────

async fn runtime_is_ready(state: &Arc<AppState>) -> bool {
    state.runtime_ready.load(Ordering::Relaxed) && is_sing_box_running(state).await
}

async fn tool_get_status(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let kernel = kernel_status(state).await;
    let (running, uptime_secs) = (kernel.running, kernel.uptime_secs);
    // Same projection as GET /api/status: kernel_status reaps a dead child
    // and clears the flag. Clash queries below still require a live process.
    let ready = state.runtime_ready.load(Ordering::Relaxed);

    let config = state.config.read().await.clone();
    let route_mode = config.route_mode;

    let warnings = runtime_warnings(state).await;
    let warning = legacy_warning(&warnings);

    // 当前节点与节点数：仅在运行时问 Clash，失败静默降级；
    // 节点数按平铺节点池计（不随 fastest_* 地区过滤收缩）
    let mut current_node = JsonValue::Null;
    let mut node_count = config.nodes.len();
    if running && ready {
        if let Ok(proxies) = fetch_proxies(state).await {
            let pool = flat_node_pool(&proxies);
            if !pool.is_empty() {
                node_count = pool.len();
            }
            if let Some((_, now)) = selector_view(&proxies) {
                current_node = now.map(JsonValue::from).unwrap_or(JsonValue::Null);
            }
        }
    }

    Ok(json!({
        "running": running,
        "ready": ready,
        "phase": state.runtime_phase(),
        "initializing": state.initializing.load(Ordering::Relaxed),
        "route_mode": route_mode,
        "node_select": config.node_select,
        "platform": if cfg!(windows) { "windows" } else { "linux" },
        "vps_supported": crate::platform::vps_supported(),
        "upgrade_supported": crate::platform::upgrade_supported(),
        "mcp": config.mcp,
        "current_node": current_node,
        "node_count": node_count,
        "uptime_secs": uptime_secs,
        "warning": warning.map(JsonValue::from).unwrap_or(JsonValue::Null),
        "warnings": warnings,
    }))
}

async fn tool_list_nodes(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let config = state.config.read().await.clone();

    // 手动节点 tag → type，用于标注 source
    let mut manual_types = std::collections::HashMap::new();
    for raw in &config.nodes {
        if let Ok(node) = serde_json::from_str::<JsonValue>(raw) {
            if let (Some(tag), node_type) = (
                node.get("tag").and_then(JsonValue::as_str),
                node.get("type").and_then(JsonValue::as_str),
            ) {
                manual_types.insert(tag.to_string(), node_type.unwrap_or("unknown").to_string());
            }
        }
    }

    let running = is_sing_box_running(state).await;
    let ready = running && state.runtime_ready.load(Ordering::Relaxed);
    if ready {
        if let Ok(proxies) = fetch_proxies(state).await {
            // 平铺节点池：不随 fastest_* 地区过滤收缩（地区外节点仍是合法 outbound）
            let pool = flat_node_pool(&proxies);
            if !pool.is_empty() {
                let now = selector_view(&proxies).and_then(|(_, now)| now);
                let nodes: Vec<JsonValue> = pool
                    .iter()
                    .map(|name| {
                        let node_type = proxies
                            .get(name)
                            .and_then(|node| node.get("type"))
                            .and_then(JsonValue::as_str)
                            .unwrap_or("unknown");
                        json!({
                            "name": name,
                            "type": manual_types.get(name).map(String::as_str).unwrap_or(node_type),
                            "source": if manual_types.contains_key(name) { "manual" } else { "subscription" },
                            "is_current": now.as_deref() == Some(name.as_str()),
                        })
                    })
                    .collect();
                return Ok(json!({ "running": true, "ready": true, "nodes": nodes }));
            }
        }
    }

    // 未就绪（或 Clash 不可达）：只能给出手动节点
    let nodes: Vec<JsonValue> = manual_types
        .iter()
        .map(|(tag, node_type)| {
            json!({
                "name": tag,
                "type": node_type,
                "source": "manual",
                "is_current": false,
            })
        })
        .collect();
    Ok(json!({
        "running": running,
        "ready": ready,
        "nodes": nodes,
        "note": "代理数据面尚未就绪，仅列出手动节点；订阅节点需就绪后可见",
    }))
}

async fn tool_switch_node(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    let name = args
        .get("name")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `name`".to_string())?;

    if state.initializing.load(Ordering::Relaxed) {
        return Err("初始化进行中，稍后再试".to_string());
    }
    if !state.config.read().await.node_select.is_manual() {
        return Err("当前是地区最快模式，由内核自动选节点；先切回手动选择再指定节点".to_string());
    }
    if !runtime_is_ready(state).await {
        return Err("服务未运行或代理数据面尚未就绪，无法切换节点".to_string());
    }

    let proxies = fetch_proxies(state)
        .await
        .map_err(|_| "Clash API 不可达".to_string())?;
    let (all, now) = selector_view(&proxies).ok_or("节点池为空".to_string())?;
    if !all.iter().any(|candidate| candidate == name) {
        return Err(format!("未知节点: {name}（用 list_nodes 查看可选节点）"));
    }
    if now.as_deref() == Some(name) {
        return Ok(json!({ "switched": name, "changed": false, "note": "已是当前节点" }));
    }

    let url = format!("{CLASH_API_BASE}/proxies/{SELECTOR_TAG}");
    let response = state
        .http_client
        .put(&url)
        .timeout(Duration::from_secs(5))
        .json(&json!({ "name": name }))
        .send()
        .await
        .map_err(|e| format!("切换请求失败: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("切换失败: Clash API 返回 {}", response.status()));
    }

    // 与面板切换同路径：持久化选择，重启后自动恢复
    let persist = crate::services::proxy::save_last_proxy(
        state,
        &LastProxy {
            group: SELECTOR_TAG.to_string(),
            name: name.to_string(),
        },
    )
    .await;

    Ok(json!({
        "switched": name,
        "changed": true,
        "persisted": persist.is_ok(),
    }))
}

async fn tool_test_delay(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    if !runtime_is_ready(state).await {
        return Err("代理数据面尚未就绪，无法测速".to_string());
    }

    if let Some(name) = args.get("name").and_then(JsonValue::as_str) {
        let delay = fetch_delay(state, name).await;
        return Ok(json!({
            "delays": { name: delay },
            "note": "延迟单位毫秒；-1 表示超时或失败",
        }));
    }

    let proxies = fetch_proxies(state)
        .await
        .map_err(|_| "Clash API 不可达".to_string())?;
    // 平铺节点池：与 list_nodes 同口径，fastest_* 地区外的节点也可测
    let all = flat_node_pool(&proxies);
    if all.is_empty() {
        return Err("节点池为空".to_string());
    }

    // 与前端批量测速一致：并发受限，避免大订阅瞬间发出数百个请求
    let results: Vec<(String, i64)> = stream::iter(all)
        .map(|name| async move {
            let delay = fetch_delay(state, &name).await;
            (name, delay)
        })
        .buffer_unordered(DELAY_CONCURRENCY)
        .collect()
        .await;

    let delays: serde_json::Map<String, JsonValue> = results
        .into_iter()
        .map(|(name, delay)| (name, json!(delay)))
        .collect();
    Ok(json!({
        "delays": JsonValue::Object(delays),
        "note": "延迟单位毫秒；-1 表示超时或失败",
    }))
}

async fn fetch_delay(state: &Arc<AppState>, name: &str) -> i64 {
    let url = format!(
        "{CLASH_API_BASE}/proxies/{}/delay?timeout={DELAY_TIMEOUT_MS}&url={DELAY_TEST_URL}",
        urlencoding::encode(name),
    );
    let response = state
        .http_client
        .get(&url)
        .timeout(DELAY_HTTP_TIMEOUT)
        .send()
        .await;
    match response {
        Ok(res) if res.status().is_success() => res
            .json::<JsonValue>()
            .await
            .ok()
            .and_then(|payload| payload.get("delay").and_then(JsonValue::as_i64))
            .filter(|delay| *delay > 0)
            .unwrap_or(-1),
        _ => -1,
    }
}

async fn tool_set_route_mode(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    let mode = args
        .get("mode")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `mode`".to_string())?;
    let requested = match mode {
        "rule" => RouteMode::Rule,
        "global" => RouteMode::Global,
        _ => return Err("Invalid params: `mode` 必须是 rule 或 global".to_string()),
    };

    if state.initializing.load(Ordering::Relaxed) {
        return Err("初始化进行中，稍后再试".to_string());
    }

    let (previous, runtime_update) = crate::services::config::apply_route_mode(state, requested)
        .await
        .map_err(|e| format!("切换路由模式失败: {e}"))?;
    let runtime_updated = runtime_update.updated();
    let changed = previous != requested;
    let note = if changed {
        "已写入易变层配置；OpenWrt/Linux 系统重启后回到 config.yaml 的启动默认值（未设置则规则分流）"
    } else {
        "未变化"
    };

    Ok(json!({
        "route_mode": mode,
        "changed": changed,
        "runtime_updated": runtime_updated,
        "started": runtime_update == RuntimeUpdate::Started,
        "reloaded": runtime_update == RuntimeUpdate::Reloaded,
        "restarted": runtime_update == RuntimeUpdate::Restarted,
        "note": note,
    }))
}

/// 与面板「节点选择」同一条链路：配置事务 + 运行时热应用（易变层落盘）。
/// 地区筛空时内核回退 manual：如实返回实际生效值（与面板提示一致）。
async fn tool_set_node_select(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    let raw = args
        .get("select")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| "Invalid params: missing `select`".to_string())?;
    let node_select = NodeSelect::parse(raw).ok_or_else(|| {
        "Invalid params: `select` 必须是 manual / fastest_hk / fastest_jp / fastest_tw / fastest_sg / fastest_us"
            .to_string()
    })?;

    if state.initializing.load(Ordering::Relaxed) {
        return Err("初始化进行中，稍后再试".to_string());
    }

    let (previous, effective, runtime_update) =
        crate::services::config::apply_node_select(state, node_select)
            .await
            .map_err(|e| format!("切换节点选择失败: {e}"))?;
    let runtime_updated = runtime_update.updated();
    let note = if !node_select.is_manual() && effective.is_manual() {
        REGION_FALLBACK
    } else if previous == node_select {
        "未变化"
    } else {
        "已保存节点选择偏好"
    };
    Ok(json!({
        "node_select": effective.as_str(),
        "requested": raw,
        "changed": previous != node_select,
        "runtime_updated": runtime_updated,
        "started": runtime_update == RuntimeUpdate::Started,
        "reloaded": runtime_update == RuntimeUpdate::Reloaded,
        "restarted": runtime_update == RuntimeUpdate::Restarted,
        "note": note,
    }))
}

/// 与面板「刷新订阅」同一条链路：真拉取 → 生成 → 校验 → 有变化才更新运行配置；
/// 全部订阅失败时保留当前运行配置。
async fn tool_refresh_subscriptions(state: &Arc<AppState>) -> Result<JsonValue, String> {
    if state.initializing.load(Ordering::Relaxed) {
        return Err("初始化进行中，稍后再试".to_string());
    }
    if state.config.read().await.subs.is_empty() {
        return Err("没有配置订阅，无可刷新".to_string());
    }

    let _config_update = state.config_update.lock().await;
    let config = state.config.read().await.clone();

    let runtime_update =
        crate::services::config::regenerate_preserving_service_state(&config, state)
            .await
            .map_err(|e| format!("刷新订阅失败: {e}"))?;
    let runtime_updated = runtime_update.updated();

    let warning = state.config_warning.lock().await.clone();
    Ok(json!({
        "refreshed": true,
        "runtime_updated": runtime_updated,
        "started": runtime_update == RuntimeUpdate::Started,
        "reloaded": runtime_update == RuntimeUpdate::Reloaded,
        "restarted": runtime_update == RuntimeUpdate::Restarted,
        "warning": warning.map(JsonValue::from).unwrap_or(JsonValue::Null),
    }))
}

async fn tool_list_rules(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let config = state.config.read().await.clone();
    let skipped_rules = state.skipped_rules.lock().await;
    let skipped_raws: HashSet<&str> = skipped_rules.iter().map(|rule| rule.raw.as_str()).collect();

    let rules: Vec<_> = config
        .custom_rules
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let mut info = crate::handlers::rules::describe_rule(index, raw);
            info.skipped = skipped_raws.contains(raw.as_str());
            info
        })
        .collect();

    Ok(json!({ "rules": serde_json::to_value(rules).unwrap_or_default() }))
}

fn pagination_value(
    args: &JsonValue,
    key: &str,
    default: usize,
    min: usize,
    max: Option<usize>,
) -> Result<usize, String> {
    let Some(value) = args.get(key) else {
        return Ok(default);
    };
    let raw = value
        .as_u64()
        .ok_or_else(|| format!("Invalid params: `{key}` 必须是非负整数"))?;
    let value =
        usize::try_from(raw).map_err(|_| format!("Invalid params: `{key}` 超出支持范围"))?;
    if value < min || max.is_some_and(|max| value > max) {
        return Err(match max {
            Some(max) => format!("Invalid params: `{key}` 必须在 {min}..={max} 范围内"),
            None => format!("Invalid params: `{key}` 不能小于 {min}"),
        });
    }
    Ok(value)
}

async fn tool_get_traffic(state: &Arc<AppState>) -> Result<JsonValue, String> {
    if !runtime_is_ready(state).await {
        return Err("代理数据面尚未就绪，无法读取实时流量".to_string());
    }

    let message = tokio::time::timeout(Duration::from_secs(3), async {
        let (mut socket, _) = tokio_tungstenite::connect_async(CLASH_TRAFFIC_WS)
            .await
            .map_err(|err| format!("连接 Clash 流量接口失败: {err}"))?;
        socket
            .next()
            .await
            .ok_or_else(|| "Clash 流量接口未返回数据".to_string())?
            .map_err(|err| format!("读取 Clash 流量失败: {err}"))
    })
    .await
    .map_err(|_| "读取实时流量超时".to_string())??;

    let payload: JsonValue = match message {
        tokio_tungstenite::tungstenite::Message::Text(text) => {
            serde_json::from_str(&text).map_err(|err| format!("流量数据格式无效: {err}"))?
        }
        tokio_tungstenite::tungstenite::Message::Binary(bytes) => {
            serde_json::from_slice(&bytes).map_err(|err| format!("流量数据格式无效: {err}"))?
        }
        _ => return Err("Clash 流量接口返回了非数据帧".to_string()),
    };
    let up = payload
        .get("up")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| "流量数据缺少 `up`".to_string())?;
    let down = payload
        .get("down")
        .and_then(JsonValue::as_u64)
        .ok_or_else(|| "流量数据缺少 `down`".to_string())?;
    Ok(json!({
        "up": up,
        "down": down,
        "unit": "bytes_per_second",
    }))
}

async fn tool_list_connections(
    state: &Arc<AppState>,
    args: &JsonValue,
) -> Result<JsonValue, String> {
    if !runtime_is_ready(state).await {
        return Err("代理数据面尚未就绪，无可用活动连接".to_string());
    }

    let payload = clash_get(state, "/connections")
        .await
        .map_err(|_| "Clash API 不可达".to_string())?;

    let connections = payload
        .get("connections")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();

    let offset = pagination_value(args, "offset", 0, 0, None)?;
    let limit = pagination_value(args, "limit", 100, 1, Some(500))?;
    let total_count = connections.len();
    let projected: Vec<JsonValue> = connections
        .iter()
        .skip(offset)
        .take(limit)
        .map(|connection| {
            let metadata = connection.get("metadata").cloned().unwrap_or(json!({}));
            let host = metadata
                .get("host")
                .and_then(JsonValue::as_str)
                .filter(|host| !host.is_empty())
                .or_else(|| metadata.get("destinationIP").and_then(JsonValue::as_str))
                .unwrap_or("");
            let chains = connection
                .get("chains")
                .and_then(JsonValue::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .collect::<Vec<_>>()
                        .join(" → ")
                })
                .unwrap_or_default();
            let rule = match (
                connection.get("rule").and_then(JsonValue::as_str),
                connection.get("rulePayload").and_then(JsonValue::as_str),
            ) {
                (Some(rule), Some(payload)) => format!("{rule} {payload}"),
                (Some(rule), None) => rule.to_string(),
                _ => String::new(),
            };
            json!({
                "host": host,
                "port": metadata.get("destinationPort").cloned().unwrap_or(JsonValue::Null),
                "network": metadata.get("network").cloned().unwrap_or(JsonValue::Null),
                "outbound": chains,
                "rule": rule,
                "download": connection.get("download").cloned().unwrap_or(json!(0)),
                "upload": connection.get("upload").cloned().unwrap_or(json!(0)),
            })
        })
        .collect();

    Ok(json!({
        "download_total": payload.get("downloadTotal").cloned().unwrap_or(json!(0)),
        "upload_total": payload.get("uploadTotal").cloned().unwrap_or(json!(0)),
        "total_count": total_count,
        "offset": offset,
        "limit": limit,
        "count": projected.len(),
        "connections": projected,
        "next_offset": if offset.saturating_add(limit) < total_count {
            json!(offset.saturating_add(limit))
        } else {
            JsonValue::Null
        },
    }))
}

#[cfg(test)]
mod tests;
