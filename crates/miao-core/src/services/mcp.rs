//! MCP（Model Context Protocol）端点核心：无状态 JSON-RPC 2.0 over POST /mcp。
//! 协议版本 2026-07-28：无握手、无会话，每个请求自描述。
//! 节点模型与面板一致：全部订阅 + 手动节点构成一个平铺节点池，没有分组概念；
//! 运行时的 sing-box selector（tag "proxy"）只是实现细节，不向 MCP 暴露。

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde_json::{json, Value as JsonValue};

use crate::error::AppResult;
use crate::models::{LastProxy, RouteMode};
use crate::services::config::apply_runtime_config_change;
use crate::state::AppState;
use crate::VERSION;

pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";

const CLASH_API_BASE: &str = "http://127.0.0.1:6262";
/// 配置模板里唯一的 selector；全部节点的容器，对外不暴露
const SELECTOR_TAG: &str = "proxy";
const DELAY_TIMEOUT_MS: u64 = 3000;
const DELAY_TEST_URL: &str = "http://www.gstatic.com/generate_204";
const DELAY_CONCURRENCY: usize = 6;

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
        "instructions": "miao 是本机/路由器的透明代理控制面。你（调用者）的出网流量很可能正经过它：停止或重启内核、切换路由模式都会造成秒级网络中断——包括你自己的连接。执行此类操作前请先向用户说明并确认。切换节点是毫秒级操作，但已建立的连接会重置。",
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

fn tools_catalog() -> JsonValue {
    json!([
        {
            "name": "get_status",
            "description": "服务状态：内核是否在跑、路由模式（分流/全局）、当前节点、运行时长、告警",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "list_nodes",
            "description": "平铺节点池：全部订阅节点与手动节点（name/type/source/is_current）。无分组概念",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "switch_node",
            "description": "切换当前节点；选择会持久化，重启后自动恢复。毫秒级完成，但已建立的连接会重置",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "节点名（来自 list_nodes）" },
                },
                "required": ["name"],
            },
        },
        {
            "name": "test_delay",
            "description": "测节点延迟（毫秒）。不传 name 则测全部节点",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "节点名；缺省测全部" },
                },
            },
        },
        {
            "name": "set_route_mode",
            "description": "切换路由模式：rule=规则分流（国内直连/国外代理），global=全局代理。会话级，不写配置文件。注意：会热重启内核，所有连接（可能包括你自己的）秒级中断，操作前请先向用户确认",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["rule", "global"] },
                },
                "required": ["mode"],
            },
        },
        {
            "name": "list_rules",
            "description": "自定义规则列表：域名/IP/端口/进程等条件到直连/代理/拦截/指定节点的映射",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "list_connections",
            "description": "当前活动连接：按站点投影出 host、出口、命中规则与流量",
            "inputSchema": { "type": "object", "properties": {} },
        },
    ])
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
        "list_nodes" => tool_list_nodes(state).await,
        "switch_node" => tool_switch_node(state, &args).await,
        "test_delay" => tool_test_delay(state, &args).await,
        "set_route_mode" => tool_set_route_mode(state, &args).await,
        "list_rules" => tool_list_rules(state).await,
        "list_connections" => tool_list_connections(state).await,
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

async fn sing_box_is_running(state: &Arc<AppState>) -> bool {
    let mut lock = state.sing_process.lock().await;
    match &mut *lock {
        Some(proc) => match proc.child.try_wait() {
            Ok(Some(_)) => {
                *lock = None;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        },
        None => false,
    }
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

// ── 工具实现 ─────────────────────────────────────────────────────────────

async fn tool_get_status(state: &Arc<AppState>) -> Result<JsonValue, String> {
    let (running, uptime_secs) = {
        let mut lock = state.sing_process.lock().await;
        match &mut *lock {
            Some(proc) => match proc.child.try_wait() {
                Ok(None) => (true, Some(proc.started_at.elapsed().as_secs())),
                Ok(Some(_)) => {
                    *lock = None;
                    (false, None)
                }
                Err(_) => (false, None),
            },
            None => (false, None),
        }
    };

    let route_mode = state
        .route_mode_override
        .read()
        .await
        .unwrap_or(RouteMode::default());
    let config = state.config.read().await.clone();

    let mut warnings: Vec<String> = Vec::new();
    if let Some(warning) = state.config_warning.lock().await.clone() {
        warnings.push(warning);
    }
    let skipped = state.skipped_rules.lock().await.len();
    if skipped > 0 {
        warnings.push(format!("{skipped} 条自定义规则因出口节点不存在已跳过"));
    }

    // 当前节点与节点数：仅在运行时问 Clash，失败静默降级
    let mut current_node = JsonValue::Null;
    let mut node_count = config.nodes.len();
    if running {
        if let Ok(proxies) = fetch_proxies(state).await {
            if let Some((all, now)) = selector_view(&proxies) {
                node_count = all.len();
                current_node = now.map(JsonValue::from).unwrap_or(JsonValue::Null);
            }
        }
    }

    Ok(json!({
        "running": running,
        "initializing": state.initializing.load(Ordering::Relaxed),
        "route_mode": route_mode,
        "adblock": config.adblock,
        "current_node": current_node,
        "node_count": node_count,
        "uptime_secs": uptime_secs,
        "warning": if warnings.is_empty() { JsonValue::Null } else { json!(warnings.join("; ")) },
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

    let running = sing_box_is_running(state).await;
    if running {
        if let Ok(proxies) = fetch_proxies(state).await {
            if let Some((all, now)) = selector_view(&proxies) {
                let nodes: Vec<JsonValue> = all
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
                return Ok(json!({ "running": true, "nodes": nodes }));
            }
        }
    }

    // 未运行（或 Clash 不可达）：只能给出手动节点
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
        "running": false,
        "nodes": nodes,
        "note": "服务未运行，仅列出手动节点；订阅节点需运行后可见",
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
    if !sing_box_is_running(state).await {
        return Err("服务未运行，无法切换节点".to_string());
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
    let persist = crate::services::proxy::save_last_proxy(&LastProxy {
        group: SELECTOR_TAG.to_string(),
        name: name.to_string(),
    })
    .await;

    Ok(json!({
        "switched": name,
        "changed": true,
        "persisted": persist.is_ok(),
    }))
}

async fn tool_test_delay(state: &Arc<AppState>, args: &JsonValue) -> Result<JsonValue, String> {
    if !sing_box_is_running(state).await {
        return Err("服务未运行，无法测速".to_string());
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
    let (all, _) = selector_view(&proxies).ok_or("节点池为空".to_string())?;

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
    let response = state.http_client.get(&url).send().await;
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

    // 与面板 set_route_mode 同一条链路：配置事务 + 运行时热应用
    let _config_update = state.config_update.lock().await;
    let was_running = sing_box_is_running(state).await;
    let old_config = state.config.read().await.clone();
    let current = state
        .route_mode_override
        .read()
        .await
        .unwrap_or(RouteMode::default());

    if current == requested {
        return Ok(json!({ "route_mode": mode, "changed": false }));
    }

    let mut old_runtime_config = old_config.clone();
    old_runtime_config.route_mode = current;
    let mut new_runtime_config = old_config.clone();
    new_runtime_config.route_mode = requested;

    apply_runtime_config_change(state, &old_runtime_config, &new_runtime_config, was_running)
        .await
        .map_err(|e| format!("切换路由模式失败: {e}"))?;

    Ok(json!({
        "route_mode": mode,
        "changed": true,
        "restarted": was_running,
        "note": "会话级状态，不写配置文件；重启后回到规则分流",
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

async fn tool_list_connections(state: &Arc<AppState>) -> Result<JsonValue, String> {
    if !sing_box_is_running(state).await {
        return Err("服务未运行，无活动连接".to_string());
    }

    let payload = clash_get(state, "/connections")
        .await
        .map_err(|_| "Clash API 不可达".to_string())?;

    let connections = payload
        .get("connections")
        .and_then(JsonValue::as_array)
        .cloned()
        .unwrap_or_default();

    let projected: Vec<JsonValue> = connections
        .iter()
        .take(100)
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
        "count": projected.len(),
        "connections": projected,
        "note": if connections.len() > 100 { "仅返回前 100 条" } else { "" },
    }))
}

#[cfg(test)]
mod tests {
    use super::{handle, MCP_PROTOCOL_VERSION};
    use crate::models::Config;
    use crate::test_support::app_state;
    use serde_json::{json, Value as JsonValue};
    use std::sync::Arc;

    fn state(config: Config) -> Arc<crate::state::AppState> {
        let state = app_state(config);
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        state
    }

    async fn call(state: &Arc<crate::state::AppState>, body: JsonValue) -> JsonValue {
        handle(state, body.to_string().as_bytes())
            .await
            .expect("request must produce a response")
    }

    #[tokio::test]
    async fn parse_error_returns_32700() {
        let response = handle(&state(Config::default()), b"not json".as_slice())
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], -32700);
        assert_eq!(response["id"], JsonValue::Null);
    }

    #[tokio::test]
    async fn notification_gets_no_response() {
        let notification = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(
            &state(Config::default()),
            notification.to_string().as_bytes()
        )
        .await
        .is_none());
    }

    #[tokio::test]
    async fn unknown_method_returns_32601() {
        let response = call(
            &state(Config::default()),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" }),
        )
        .await;
        assert_eq!(response["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn discover_reports_protocol_version_and_tools_capability() {
        for method in ["initialize", "server/discover"] {
            let response = call(
                &state(Config::default()),
                json!({ "jsonrpc": "2.0", "id": 1, "method": method }),
            )
            .await;
            let result = &response["result"];
            assert_eq!(result["protocolVersion"], MCP_PROTOCOL_VERSION);
            assert_eq!(result["serverInfo"]["name"], "miao");
            assert!(result["capabilities"]["tools"].is_object());
            // 调用者须知：流量可能经过本代理，破坏性操作会自断其网
            assert!(result["instructions"]
                .as_str()
                .unwrap()
                .contains("网络中断"));
        }
    }

    #[tokio::test]
    async fn tools_list_covers_the_flat_node_model() {
        let response = call(
            &state(Config::default()),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        )
        .await;
        let tools = response["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        for expected in [
            "get_status",
            "list_nodes",
            "switch_node",
            "test_delay",
            "set_route_mode",
            "list_rules",
            "list_connections",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
        assert!(response["result"]["ttlMs"].is_number());
    }

    #[tokio::test]
    async fn unknown_tool_returns_32602_style_error() {
        let response = call(
            &state(Config::default()),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "explode", "arguments": {} },
            }),
        )
        .await;
        // 未知工具走 isError 结果（MCP 惯例），不是协议错误
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Unknown tool"));
    }

    #[tokio::test]
    async fn get_status_works_when_stopped_without_network() {
        let response = call(
            &state(Config::default()),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "get_status", "arguments": {} },
            }),
        )
        .await;
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let payload: JsonValue = serde_json::from_str(text).unwrap();
        assert_eq!(payload["running"], false);
        assert_eq!(payload["route_mode"], "rule");
        assert!(payload["current_node"].is_null());
    }

    #[tokio::test]
    async fn list_nodes_falls_back_to_manual_nodes_when_stopped() {
        let config = Config {
            nodes: vec![
                r#"{"type":"hysteria2","tag":"手动节点A","server":"a.example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#.to_string(),
            ],
            ..Default::default()
        };
        let response = call(
            &state(config),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "list_nodes", "arguments": {} },
            }),
        )
        .await;
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let payload: JsonValue = serde_json::from_str(text).unwrap();
        assert_eq!(payload["running"], false);
        let nodes = payload["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["name"], "手动节点A");
        assert_eq!(nodes[0]["source"], "manual");
        assert_eq!(nodes[0]["is_current"], false);
    }

    #[tokio::test]
    async fn switch_node_requires_running_service() {
        let response = call(
            &state(Config::default()),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "switch_node", "arguments": { "name": "任意节点" } },
            }),
        )
        .await;
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("服务未运行"));
    }

    #[tokio::test]
    async fn switch_node_validates_arguments() {
        let response = call(
            &state(Config::default()),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "switch_node", "arguments": {} },
            }),
        )
        .await;
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("missing `name`"));
    }

    #[tokio::test]
    async fn set_route_mode_validates_mode() {
        let response = call(
            &state(Config::default()),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "set_route_mode", "arguments": { "mode": "moon" } },
            }),
        )
        .await;
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("rule 或 global"));
    }

    #[tokio::test]
    async fn list_rules_returns_structured_entries() {
        let config = Config {
            custom_rules: vec![
                r#"{"process_name":"curl","action":"route","outbound":"direct"}"#.to_string(),
            ],
            ..Default::default()
        };
        let response = call(
            &state(config),
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": "list_rules", "arguments": {} },
            }),
        )
        .await;
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let payload: JsonValue = serde_json::from_str(text).unwrap();
        let rules = payload["rules"].as_array().unwrap();
        assert_eq!(rules[0]["field"], "process_name");
        assert_eq!(rules[0]["value"], "curl");
        assert_eq!(rules[0]["target"], "direct");
        assert_eq!(rules[0]["skipped"], false);
    }
}
