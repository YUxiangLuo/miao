use std::{
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    http::{header, uri::Authority, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt},
    sync::{mpsc, watch},
    time::{Duration, Instant},
};

use crate::{
    models::{AgentConfigRequest, AgentStatusData},
    responses::{status_error, success, HandlerResult},
    services::agent::{
        agent_status, agent_unsupported_reason, ensure_pi_installed, load_agent_config,
        save_agent_config, spawn_pi_process, stop_pi_process, AgentPreparationEvent, PiProcess,
    },
    state::AppState,
};

const MAX_CLIENT_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_PROMPT_CHARS: usize = 8_000;
const MAX_RPC_RECORD_BYTES: usize = 4 * 1024 * 1024;
const RPC_START_TIMEOUT: Duration = Duration::from_secs(15);
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum AgentClientCommand {
    Prompt { message: String },
    Abort,
}

struct ActiveSessionGuard(Arc<AppState>);

impl Drop for ActiveSessionGuard {
    fn drop(&mut self) {
        self.0.agent_session_active.store(false, Ordering::Release);
    }
}

fn request_is_loopback(address: &SocketAddr) -> bool {
    address.ip().is_loopback()
}

fn host_without_ipv6_brackets(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
}

fn authority_is_loopback(authority: &Authority) -> bool {
    let host = host_without_ipv6_brackets(authority.host());
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn browser_origin_is_allowed(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(authority) = host.parse::<Authority>() else {
        return false;
    };
    if !authority_is_loopback(&authority) {
        return false;
    }

    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };
    let Ok(origin) = url::Url::parse(origin) else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https") {
        return false;
    }
    let Some(origin_host) = origin.host_str() else {
        return false;
    };
    let host_matches = host_without_ipv6_brackets(origin_host)
        .eq_ignore_ascii_case(host_without_ipv6_brackets(authority.host()));
    let expected_port = authority
        .port_u16()
        .or_else(|| (origin.scheme() == "http").then_some(80))
        .or_else(|| (origin.scheme() == "https").then_some(443));
    host_matches && origin.port_or_known_default() == expected_port
}

pub async fn get_agent_status(
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
) -> HandlerResult<AgentStatusData> {
    if !request_is_loopback(&address) {
        return Err(status_error(
            StatusCode::FORBIDDEN,
            "Pi Agent MVP 仅允许从本机访问",
        ));
    }

    match agent_status(&state).await {
        Ok(status) => Ok(success("Agent status loaded", status)),
        Err(err) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, err)),
    }
}

pub async fn configure_agent(
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<AgentConfigRequest>,
) -> HandlerResult<AgentStatusData> {
    if !request_is_loopback(&address) || !browser_origin_is_allowed(&headers) {
        return Err(status_error(
            StatusCode::FORBIDDEN,
            "Pi Agent MVP 仅允许从本机访问",
        ));
    }

    if let Some(reason) = agent_unsupported_reason() {
        return Err(status_error(StatusCode::BAD_REQUEST, reason));
    }

    save_agent_config(&state.config_path, request)
        .await
        .map_err(|err| status_error(StatusCode::BAD_REQUEST, err))?;
    let status = agent_status(&state)
        .await
        .map_err(|err| status_error(StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(success("Agent provider configured", status))
}

pub async fn agent_websocket(
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !request_is_loopback(&address) || !browser_origin_is_allowed(&headers) {
        return status_error::<()>(StatusCode::FORBIDDEN, "Pi Agent MVP 仅允许从本机访问")
            .into_response();
    }

    let status = match agent_status(&state).await {
        Ok(status) => status,
        Err(err) => {
            return status_error::<()>(StatusCode::INTERNAL_SERVER_ERROR, err).into_response()
        }
    };
    if !status.supported {
        return status_error::<()>(
            StatusCode::BAD_REQUEST,
            status
                .reason
                .unwrap_or_else(|| "当前环境不支持 Pi Agent".to_string()),
        )
        .into_response();
    }
    if !status.configured {
        return status_error::<()>(StatusCode::BAD_REQUEST, "请先配置 AI Provider").into_response();
    }

    ws.max_frame_size(MAX_CLIENT_MESSAGE_BYTES)
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |socket| run_agent_socket(socket, state))
        .into_response()
}

async fn send_json(socket: &mut WebSocket, value: Value) -> bool {
    match serde_json::to_string(&value) {
        Ok(text) => socket.send(Message::Text(text.into())).await.is_ok(),
        Err(_) => false,
    }
}

async fn send_phase(socket: &mut WebSocket, phase: &str, message: &str) -> bool {
    send_json(
        socket,
        json!({ "type": "phase", "phase": phase, "message": message }),
    )
    .await
}

async fn wait_for_agent_shutdown(shutdown: &mut watch::Receiver<bool>) {
    let _ = shutdown.wait_for(|requested| *requested).await;
}

async fn prepare_pi(
    socket: &mut WebSocket,
    state: &Arc<AppState>,
    shutdown: &mut watch::Receiver<bool>,
) -> Option<std::path::PathBuf> {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let install = ensure_pi_installed(state, &event_tx);
    tokio::pin!(install);

    loop {
        tokio::select! {
            _ = wait_for_agent_shutdown(shutdown) => return None,
            result = &mut install => {
                return match result {
                    Ok(path) => {
                        send_phase(socket, "prepared", "Pi Agent 准备完成").await.then_some(path)
                    }
                    Err(err) => {
                        let _ = send_json(socket, json!({ "type": "error", "message": err.to_string() })).await;
                        None
                    }
                };
            }
            event = event_rx.recv() => {
                let Some(event) = event else { continue };
                let (phase, message, total_bytes) = match event {
                    AgentPreparationEvent::Checking => ("checking", "正在检查运行环境…", None),
                    AgentPreparationEvent::Downloading { total_bytes } => (
                        "downloading",
                        "首次使用，正在下载 Pi Agent…",
                        Some(total_bytes),
                    ),
                    AgentPreparationEvent::Verifying => ("verifying", "正在验证 Pi Agent…", None),
                    AgentPreparationEvent::Extracting => ("extracting", "正在准备 Pi Agent…", None),
                };
                let mut value = json!({ "type": "phase", "phase": phase, "message": message });
                if let Some(total_bytes) = total_bytes {
                    value["total_bytes"] = json!(total_bytes);
                }
                let _ = send_json(socket, value).await;
            }
        }
    }
}

async fn write_rpc(process: &mut PiProcess, value: Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(&value).map_err(|err| err.to_string())?;
    bytes.push(b'\n');
    process
        .stdin
        .write_all(&bytes)
        .await
        .map_err(|err| format!("Failed to write Pi RPC command: {err}"))?;
    process
        .stdin
        .flush()
        .await
        .map_err(|err| format!("Failed to flush Pi RPC command: {err}"))
}

async fn read_rpc(process: &mut PiProcess) -> Result<Option<Value>, String> {
    let mut record = Vec::new();
    loop {
        let (consumed, finished) = {
            let available = process
                .stdout
                .fill_buf()
                .await
                .map_err(|err| format!("Failed to read Pi RPC output: {err}"))?;
            if available.is_empty() {
                if record.is_empty() {
                    return Ok(None);
                }
                return Err("Pi RPC returned an unterminated record".to_string());
            }

            let newline = available.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(available.len(), |position| position + 1);
            let payload_length = newline.unwrap_or(consumed);
            if record.len().saturating_add(payload_length) > MAX_RPC_RECORD_BYTES {
                return Err("Pi RPC record exceeded the safety limit".to_string());
            }
            record.extend_from_slice(&available[..payload_length]);
            (consumed, newline.is_some())
        };
        process.stdout.consume(consumed);
        if finished {
            break;
        }
    }

    if record.last() == Some(&b'\r') {
        record.pop();
    }
    serde_json::from_slice(&record)
        .map(Some)
        .map_err(|err| format!("Pi RPC returned invalid JSON: {err}"))
}

fn model_from_state_response(value: &Value) -> Option<(&str, &str)> {
    let model = value.get("data")?.get("model")?;
    Some((model.get("provider")?.as_str()?, model.get("id")?.as_str()?))
}

async fn initialize_rpc(socket: &mut WebSocket, process: &mut PiProcess) -> bool {
    if let Err(message) =
        write_rpc(process, json!({ "id": "miao-state", "type": "get_state" })).await
    {
        let _ = send_json(socket, json!({ "type": "error", "message": message })).await;
        return false;
    }

    let response = tokio::time::timeout(RPC_START_TIMEOUT, async {
        loop {
            match read_rpc(process).await? {
                Some(value) if value.get("id").and_then(Value::as_str) == Some("miao-state") => {
                    return Ok::<Value, String>(value)
                }
                Some(_) => continue,
                None => return Err("Pi Agent exited during startup".to_string()),
            }
        }
    })
    .await;

    match response {
        Ok(Ok(value)) if value.get("success").and_then(Value::as_bool) == Some(true) => {
            let Some((provider, model)) = model_from_state_response(&value) else {
                let _ = send_json(
                    socket,
                    json!({ "type": "error", "message": "当前 Provider 没有可用模型，请重新配置模型 ID" }),
                )
                .await;
                return false;
            };
            send_json(
                socket,
                json!({ "type": "ready", "provider": provider, "model": model }),
            )
            .await
        }
        Ok(Ok(_)) => {
            let _ = send_json(
                socket,
                json!({
                    "type": "error",
                    "message": "Pi Agent 初始化失败，请检查 Provider 和模型配置"
                }),
            )
            .await;
            false
        }
        Ok(Err(message)) => {
            let _ = send_json(socket, json!({ "type": "error", "message": message })).await;
            false
        }
        Err(_) => {
            let _ = send_json(
                socket,
                json!({ "type": "error", "message": "Pi Agent 启动超时" }),
            )
            .await;
            false
        }
    }
}

fn assistant_text(message: &Value) -> Option<String> {
    if message.get("role")?.as_str()? != "assistant" {
        return None;
    }
    let content = message.get("content")?.as_array()?;
    let text = content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<String>();
    Some(text)
}

enum RpcForward {
    None,
    Send(Value),
    Fatal(&'static str),
}

fn safe_provider_error(message: Option<&str>) -> &'static str {
    let message = message.unwrap_or_default().to_ascii_lowercase();
    if message.contains("401") || message.contains("unauthorized") || message.contains("api key") {
        "Provider 认证失败，请检查 API Key"
    } else if message.contains("404") || message.contains("model") {
        "Provider 找不到指定模型，请检查模型 ID"
    } else if message.contains("429") || message.contains("rate limit") {
        "Provider 请求过于频繁，请稍后重试"
    } else {
        "Provider 请求失败，请检查 Provider 配置或网络连接"
    }
}

fn map_rpc_event(value: &Value) -> RpcForward {
    match value.get("type").and_then(Value::as_str) {
        Some("message_update") => {
            let event = &value["assistantMessageEvent"];
            if event.get("type").and_then(Value::as_str) == Some("text_delta") {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    return RpcForward::Send(json!({ "type": "text_delta", "delta": delta }));
                }
            }
            RpcForward::None
        }
        Some("message_end") => {
            let message = &value["message"];
            if message.get("role").and_then(Value::as_str) == Some("assistant")
                && message.get("stopReason").and_then(Value::as_str) == Some("error")
            {
                RpcForward::Send(json!({
                    "type": "request_error",
                    "message": safe_provider_error(message.get("errorMessage").and_then(Value::as_str))
                }))
            } else {
                assistant_text(message)
                    .map(|text| RpcForward::Send(json!({ "type": "message_end", "text": text })))
                    .unwrap_or(RpcForward::None)
            }
        }
        Some("agent_start") => RpcForward::Send(json!({ "type": "working" })),
        Some("agent_settled") => RpcForward::Send(json!({ "type": "settled" })),
        Some("auto_retry_start") => RpcForward::Send(json!({
            "type": "notice",
            "message": "Provider 暂时不可用，Pi 正在自动重试…"
        })),
        Some("response") if value.get("success").and_then(Value::as_bool) == Some(false) => {
            RpcForward::Send(json!({
                "type": "request_error",
                "message": "Pi Agent 拒绝了当前请求，请稍后重试"
            }))
        }
        Some(kind)
            if kind.starts_with("tool_execution_") || kind.starts_with("bash_execution_") =>
        {
            RpcForward::Fatal("Pi Agent attempted an unexpected tool execution")
        }
        _ => RpcForward::None,
    }
}

fn parse_client_command(text: &str) -> Result<AgentClientCommand, &'static str> {
    if text.len() > MAX_CLIENT_MESSAGE_BYTES {
        return Err("消息过长");
    }
    serde_json::from_str(text).map_err(|_| "不支持的助手命令")
}

async fn send_request_error(socket: &mut WebSocket, message: &str) {
    let _ = send_json(
        socket,
        json!({ "type": "request_error", "message": message }),
    )
    .await;
}

async fn handle_client_message(
    socket: &mut WebSocket,
    process: &mut PiProcess,
    message: Message,
    streaming: &mut bool,
    next_id: &mut u64,
) -> bool {
    match message {
        Message::Text(text) => match parse_client_command(&text) {
            Ok(AgentClientCommand::Prompt { message }) => {
                let message = message.trim();
                if message.is_empty() {
                    send_request_error(socket, "请输入消息").await;
                } else if message.chars().count() > MAX_PROMPT_CHARS {
                    send_request_error(socket, "消息不能超过 8000 个字符").await;
                } else if *streaming {
                    let _ = send_json(
                        socket,
                        json!({ "type": "notice", "message": "请等待当前回复完成或先停止生成" }),
                    )
                    .await;
                } else {
                    *next_id += 1;
                    let id = format!("prompt-{next_id}");
                    *streaming = true;
                    if let Err(message) = write_rpc(
                        process,
                        json!({ "id": id, "type": "prompt", "message": message }),
                    )
                    .await
                    {
                        *streaming = false;
                        let _ =
                            send_json(socket, json!({ "type": "error", "message": message })).await;
                        return false;
                    }
                }
                true
            }
            Ok(AgentClientCommand::Abort) => {
                if let Err(message) = write_rpc(process, json!({ "type": "abort" })).await {
                    let _ = send_json(socket, json!({ "type": "error", "message": message })).await;
                    return false;
                }
                true
            }
            Err(message) => {
                send_request_error(socket, message).await;
                true
            }
        },
        Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.is_ok(),
        Message::Pong(_) => true,
        Message::Close(_) => false,
        Message::Binary(_) => {
            send_request_error(socket, "暂不支持二进制消息").await;
            true
        }
    }
}

async fn chat_loop(
    socket: &mut WebSocket,
    process: &mut PiProcess,
    shutdown: &mut watch::Receiver<bool>,
) {
    let idle = tokio::time::sleep(SESSION_IDLE_TIMEOUT);
    tokio::pin!(idle);
    let mut streaming = false;
    let mut next_id = 0_u64;

    loop {
        tokio::select! {
            client_message = socket.next() => {
                let Some(Ok(message)) = client_message else { break };
                idle.as_mut().reset(Instant::now() + SESSION_IDLE_TIMEOUT);
                if !handle_client_message(socket, process, message, &mut streaming, &mut next_id).await {
                    break;
                }
            }
            rpc = read_rpc(process) => {
                match rpc {
                    Ok(Some(value)) => match map_rpc_event(&value) {
                        RpcForward::None => {}
                        RpcForward::Send(message) => {
                            if matches!(
                                message.get("type").and_then(Value::as_str),
                                Some("settled" | "request_error")
                            ) {
                                streaming = false;
                            }
                            if !send_json(socket, message).await { break; }
                        }
                        RpcForward::Fatal(message) => {
                            let _ = send_json(socket, json!({ "type": "error", "message": message })).await;
                            break;
                        }
                    },
                    Ok(None) => {
                        let _ = send_json(socket, json!({ "type": "error", "message": "Pi Agent 已退出" })).await;
                        break;
                    }
                    Err(message) => {
                        let _ = send_json(socket, json!({ "type": "error", "message": message })).await;
                        break;
                    }
                }
            }
            _ = wait_for_agent_shutdown(shutdown) => break,
            _ = &mut idle => {
                let _ = send_json(socket, json!({ "type": "error", "message": "助手因长时间未操作而关闭" })).await;
                break;
            }
        }
    }
}

async fn run_agent_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut shutdown = state.agent_shutdown.subscribe();
    if *shutdown.borrow() {
        let _ = socket.close().await;
        return;
    }

    if state
        .agent_session_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        let _ = send_json(
            &mut socket,
            json!({ "type": "error", "message": "已有一个助手会话正在运行" }),
        )
        .await;
        let _ = socket.close().await;
        return;
    }
    let _active_guard = ActiveSessionGuard(state.clone());

    let Some(binary) = prepare_pi(&mut socket, &state, &mut shutdown).await else {
        let _ = socket.close().await;
        return;
    };
    let config = match load_agent_config(&state.config_path).await {
        Ok(Some(config)) => config,
        Ok(None) => {
            let _ = send_json(
                &mut socket,
                json!({ "type": "error", "message": "请先配置 AI Provider" }),
            )
            .await;
            return;
        }
        Err(err) => {
            let _ = send_json(
                &mut socket,
                json!({ "type": "error", "message": err.to_string() }),
            )
            .await;
            return;
        }
    };

    let _ = send_phase(&mut socket, "starting", "正在启动 Pi Agent…").await;
    let mut process = match spawn_pi_process(&binary, &config).await {
        Ok(process) => process,
        Err(err) => {
            let _ = send_json(
                &mut socket,
                json!({ "type": "error", "message": err.to_string() }),
            )
            .await;
            return;
        }
    };

    let initialized = tokio::select! {
        _ = wait_for_agent_shutdown(&mut shutdown) => false,
        initialized = initialize_rpc(&mut socket, &mut process) => initialized,
    };
    if initialized {
        chat_loop(&mut socket, &mut process, &mut shutdown).await;
    }
    stop_pi_process(&mut process).await;
    let _ = socket.close().await;
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use axum::{
        extract::{ConnectInfo, State},
        http::{header, HeaderMap, StatusCode},
    };
    use serde_json::json;

    use super::{
        browser_origin_is_allowed, get_agent_status, map_rpc_event, parse_client_command,
        request_is_loopback, RpcForward,
    };
    use crate::{models::Config, test_support::app_state};

    #[test]
    fn agent_access_is_restricted_to_loopback_peers() {
        let local = ConnectInfo(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234));
        let remote = ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            1234,
        ));

        assert!(request_is_loopback(&local.0));
        assert!(!request_is_loopback(&remote.0));
    }

    #[tokio::test]
    async fn remote_status_requests_are_forbidden() {
        let remote = ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
            1234,
        ));
        let result = get_agent_status(remote, State(app_state(Config::default()))).await;

        let status = match result {
            Ok(_) => panic!("remote agent status request was allowed"),
            Err((status, _)) => status,
        };
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn browser_origin_must_match_a_loopback_host() {
        let mut valid = HeaderMap::new();
        valid.insert(header::HOST, "127.0.0.1:6161".parse().unwrap());
        valid.insert(header::ORIGIN, "http://127.0.0.1:6161".parse().unwrap());
        assert!(browser_origin_is_allowed(&valid));

        let mut localhost = HeaderMap::new();
        localhost.insert(header::HOST, "localhost:6161".parse().unwrap());
        localhost.insert(header::ORIGIN, "http://localhost:6161".parse().unwrap());
        assert!(browser_origin_is_allowed(&localhost));

        let mut ipv6 = HeaderMap::new();
        ipv6.insert(header::HOST, "[::1]:6161".parse().unwrap());
        ipv6.insert(header::ORIGIN, "http://[::1]:6161".parse().unwrap());
        assert!(browser_origin_is_allowed(&ipv6));
        assert!(request_is_loopback(&SocketAddr::new(
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            1234,
        )));

        let mut cross_site = valid.clone();
        cross_site.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!browser_origin_is_allowed(&cross_site));

        let mut rebound = valid;
        rebound.insert(header::HOST, "evil.example:6161".parse().unwrap());
        rebound.insert(header::ORIGIN, "http://evil.example:6161".parse().unwrap());
        assert!(!browser_origin_is_allowed(&rebound));
    }

    #[test]
    fn browser_protocol_never_accepts_raw_pi_bash_commands() {
        assert!(parse_client_command(r#"{"type":"bash","command":"id"}"#).is_err());
        assert!(
            parse_client_command(r#"{"type":"prompt","message":"hello","command":"id"}"#).is_err()
        );
        assert!(parse_client_command(r#"{"type":"prompt","message":"hello"}"#).is_ok());
        assert!(parse_client_command(r#"{"type":"abort"}"#).is_ok());
    }

    #[test]
    fn rpc_mapper_only_forwards_text_and_treats_tools_as_fatal() {
        let delta = json!({
            "type": "message_update",
            "assistantMessageEvent": { "type": "text_delta", "delta": "hello" }
        });
        match map_rpc_event(&delta) {
            RpcForward::Send(value) => assert_eq!(value["delta"], "hello"),
            _ => panic!("text delta was not forwarded"),
        }

        let provider_error = json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "content": [],
                "stopReason": "error",
                "errorMessage": "401 Incorrect API key: secret-value"
            }
        });
        match map_rpc_event(&provider_error) {
            RpcForward::Send(value) => {
                assert_eq!(value["type"], "request_error");
                assert_eq!(value["message"], "Provider 认证失败，请检查 API Key");
                assert!(!value.to_string().contains("secret-value"));
            }
            _ => panic!("provider error was not safely forwarded"),
        }

        assert!(matches!(
            map_rpc_event(&json!({ "type": "tool_execution_end" })),
            RpcForward::Fatal(_)
        ));
        assert!(matches!(
            map_rpc_event(&json!({ "type": "bash_execution_update" })),
            RpcForward::Fatal(_)
        ));
    }
}
