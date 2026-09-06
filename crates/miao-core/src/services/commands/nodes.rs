use serde_json::{json, Map, Value as JsonValue};
use std::sync::Arc;
use tracing::warn;

use super::{
    command_error, success, success_no_data, CommandErrorKind, CommandReply, CommandResult,
};
use crate::models::{
    BatchNodeAdded, BatchNodeFailure, BatchNodeRequest, BatchNodeResult, DeleteNodeRequest,
    NodeInfo, NodeRequest,
};
use crate::services::config::{known_rule_targets, ConfigEdit};
use crate::services::node_parser::parse_node_json;
use crate::state::AppState;
use crate::validation::{non_empty, Validator};

fn insert_optional_string(obj: &mut Map<String, JsonValue>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        obj.insert(key.to_string(), json!(value));
    }
}

fn base_outbound(typ: &str, req: &NodeRequest) -> Map<String, JsonValue> {
    let mut obj = Map::new();
    obj.insert("type".to_string(), json!(typ));
    obj.insert("tag".to_string(), json!(req.tag.trim()));
    obj.insert("server".to_string(), json!(req.server.trim()));
    obj.insert("server_port".to_string(), json!(req.server_port));
    obj
}

fn request_password(req: &NodeRequest) -> &str {
    req.password.as_deref().unwrap_or_default().trim()
}

fn build_tls(req: &NodeRequest, default_enabled: bool, force_enabled: bool) -> Option<JsonValue> {
    let enabled = force_enabled
        || req.tls_enabled.unwrap_or(default_enabled)
        || non_empty(&req.reality_public_key).is_some();
    if !enabled {
        return None;
    }

    let mut tls = Map::new();
    tls.insert("enabled".to_string(), json!(true));
    tls.insert(
        "insecure".to_string(),
        json!(req.skip_cert_verify.unwrap_or(false)),
    );
    insert_optional_string(&mut tls, "server_name", non_empty(&req.sni));

    if let Some(alpn) = req.alpn.as_ref().map(|values| {
        values
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    }) {
        if !alpn.is_empty() {
            tls.insert("alpn".to_string(), json!(alpn));
        }
    }

    if let Some(fingerprint) = non_empty(&req.client_fingerprint) {
        let fingerprint = fingerprint.to_ascii_lowercase();
        if fingerprint != "none" {
            tls.insert(
                "utls".to_string(),
                json!({
                    "enabled": true,
                    "fingerprint": fingerprint
                }),
            );
        }
    }

    if let Some(public_key) = non_empty(&req.reality_public_key) {
        let mut reality = Map::new();
        reality.insert("enabled".to_string(), json!(true));
        reality.insert("public_key".to_string(), json!(public_key));
        insert_optional_string(&mut reality, "short_id", non_empty(&req.reality_short_id));
        tls.insert("reality".to_string(), JsonValue::Object(reality));
    }

    Some(JsonValue::Object(tls))
}

fn build_transport(req: &NodeRequest) -> Option<JsonValue> {
    let transport_type = non_empty(&req.transport_type).unwrap_or("tcp");
    match transport_type {
        "tcp" => None,
        "ws" => {
            let mut transport = Map::new();
            transport.insert("type".to_string(), json!("ws"));
            insert_optional_string(&mut transport, "path", non_empty(&req.transport_path));
            if let Some(host) = non_empty(&req.transport_host) {
                transport.insert("headers".to_string(), json!({ "Host": host }));
            }
            Some(JsonValue::Object(transport))
        }
        "grpc" => {
            let mut transport = Map::new();
            transport.insert("type".to_string(), json!("grpc"));
            insert_optional_string(
                &mut transport,
                "service_name",
                non_empty(&req.grpc_service_name),
            );
            Some(JsonValue::Object(transport))
        }
        "http" | "h2" => {
            let mut transport = Map::new();
            transport.insert("type".to_string(), json!("http"));
            insert_optional_string(&mut transport, "path", non_empty(&req.transport_path));
            if let Some(host) = non_empty(&req.transport_host) {
                transport.insert("host".to_string(), json!([host]));
            }
            Some(JsonValue::Object(transport))
        }
        _ => None,
    }
}

pub(crate) fn build_node_value(req: &NodeRequest, node_type: &str) -> JsonValue {
    match node_type {
        "anytls" => {
            let mut obj = base_outbound("anytls", req);
            obj.insert("password".to_string(), json!(request_password(req)));
            obj.insert(
                "tls".to_string(),
                build_tls(req, true, true).expect("AnyTLS TLS is always enabled"),
            );
            JsonValue::Object(obj)
        }
        "ss" => {
            let mut obj = base_outbound("shadowsocks", req);
            obj.insert(
                "method".to_string(),
                json!(non_empty(&req.cipher).unwrap_or("2022-blake3-aes-128-gcm")),
            );
            obj.insert("password".to_string(), json!(request_password(req)));
            JsonValue::Object(obj)
        }
        "vmess" => {
            let mut obj = base_outbound("vmess", req);
            obj.insert(
                "uuid".to_string(),
                json!(non_empty(&req.uuid).unwrap_or_default()),
            );
            obj.insert(
                "security".to_string(),
                json!(non_empty(&req.cipher).unwrap_or("auto")),
            );
            obj.insert("alter_id".to_string(), json!(req.alter_id.unwrap_or(0)));
            insert_optional_string(&mut obj, "packet_encoding", non_empty(&req.packet_encoding));
            if let Some(tls) = build_tls(req, false, false) {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_transport(req) {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "vless" => {
            let mut obj = base_outbound("vless", req);
            obj.insert(
                "uuid".to_string(),
                json!(non_empty(&req.uuid).unwrap_or_default()),
            );
            insert_optional_string(&mut obj, "flow", non_empty(&req.flow));
            insert_optional_string(&mut obj, "packet_encoding", non_empty(&req.packet_encoding));
            if let Some(tls) = build_tls(req, true, false) {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_transport(req) {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "trojan" => {
            let mut obj = base_outbound("trojan", req);
            obj.insert("password".to_string(), json!(request_password(req)));
            if let Some(tls) = build_tls(req, true, true) {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_transport(req) {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "tuic" => {
            let mut obj = base_outbound("tuic", req);
            obj.insert(
                "uuid".to_string(),
                json!(non_empty(&req.uuid).unwrap_or_default()),
            );
            obj.insert("password".to_string(), json!(request_password(req)));
            obj.insert(
                "congestion_control".to_string(),
                json!(non_empty(&req.tuic_congestion_control).unwrap_or("cubic")),
            );
            obj.insert(
                "udp_relay_mode".to_string(),
                json!(non_empty(&req.tuic_udp_relay_mode).unwrap_or("native")),
            );
            if req.tuic_zero_rtt.unwrap_or(false) {
                obj.insert("zero_rtt_handshake".to_string(), json!(true));
            }
            obj.insert(
                "tls".to_string(),
                build_tls(req, true, true).expect("TUIC TLS is always enabled"),
            );
            JsonValue::Object(obj)
        }
        _ => {
            let mut obj = base_outbound("hysteria2", req);
            obj.insert("password".to_string(), json!(request_password(req)));
            obj.insert(
                "tls".to_string(),
                build_tls(req, true, true).expect("Hysteria2 TLS is always enabled"),
            );
            if let Some(obfs_type) = non_empty(&req.obfs_type) {
                obj.insert(
                    "obfs".to_string(),
                    json!({
                        "type": obfs_type,
                        "password": non_empty(&req.obfs_password).unwrap_or_default()
                    }),
                );
            }
            JsonValue::Object(obj)
        }
    }
}

fn validated_node_json(req: &NodeRequest) -> Result<(String, String), String> {
    Validator::validate_node_request(req)?;
    let node_type = req.node_type.as_deref().unwrap_or("hysteria2");
    let tag = req.tag.trim().to_string();
    let node_json = serde_json::to_string(&build_node_value(req, node_type))
        .map_err(|e| format!("Failed to serialize node: {e}"))?;
    Ok((tag, node_json))
}

pub async fn get_nodes(state: Arc<AppState>) -> CommandReply<Vec<NodeInfo>> {
    let config = state.config.read().await;

    let mut nodes = Vec::new();
    let mut parse_errors = Vec::new();

    for (idx, node_str) in config.nodes.iter().enumerate() {
        match parse_node_json(node_str) {
            Ok((display_info, _)) => {
                nodes.push(NodeInfo {
                    tag: display_info.tag,
                    server: display_info.server,
                    server_port: display_info.server_port,
                    node_type: display_info.node_type,
                    sni: display_info.sni,
                });
            }
            Err(e) => {
                let error_msg = format!("Node #{}: {}", idx, e);
                warn!("[get_nodes] {}", error_msg);
                parse_errors.push(error_msg);
            }
        }
    }

    // 如果有解析错误，记录到日志但不影响返回有效节点
    if !parse_errors.is_empty() {
        warn!("[get_nodes] Skipped {} invalid node(s)", parse_errors.len());
    }

    success("Nodes loaded", nodes)
}

pub async fn add_node(state: Arc<AppState>, req: NodeRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    let (tag, node_json) =
        validated_node_json(&req).map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;

    let mut edit = ConfigEdit::begin(&state).await;

    // 检查所有现有和保留的 runtime tag，防止新增手动节点静默接管订阅规则。
    let req_tag_lower = tag.to_lowercase();
    for existing_tag in known_rule_targets(edit.original(), &state).await {
        if existing_tag.to_lowercase() == req_tag_lower {
            return Err(command_error(
                CommandErrorKind::InvalidInput,
                format!(
                    "标签 '{}' 与已存在或保留的节点 '{}' 重复（不区分大小写）",
                    tag, existing_tag
                ),
            ));
        }
    }

    edit.candidate.nodes.push(node_json);

    match edit.commit().await {
        Ok(_) => Ok(success_no_data("Node added")),
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

pub async fn import_nodes(
    state: Arc<AppState>,
    req: BatchNodeRequest,
) -> CommandResult<BatchNodeResult> {
    super::ensure_initialized(&state)?;

    let mut edit = ConfigEdit::begin(&state).await;
    let existing = known_rule_targets(edit.original(), &state).await;
    let mut used: std::collections::HashSet<String> =
        existing.into_iter().map(|tag| tag.to_lowercase()).collect();
    let mut added = Vec::new();
    let mut failed = Vec::new();

    for (index, node) in req.nodes.iter().enumerate() {
        let display_tag = node.tag.trim().to_string();
        match validated_node_json(node) {
            Ok((tag, node_json)) => {
                let normalized = tag.to_lowercase();
                if used.contains(&normalized) {
                    failed.push(BatchNodeFailure {
                        index,
                        tag,
                        message: "标签与已存在、保留或本批次节点重复（不区分大小写）".to_string(),
                    });
                    continue;
                }
                used.insert(normalized);
                edit.candidate.nodes.push(node_json);
                added.push(BatchNodeAdded { index, tag });
            }
            Err(message) => failed.push(BatchNodeFailure {
                index,
                tag: display_tag,
                message,
            }),
        }
    }

    if added.is_empty() {
        return Ok(success("No nodes added", BatchNodeResult { added, failed }));
    }

    edit.commit()
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    Ok(success(
        format!("Added {} nodes", added.len()),
        BatchNodeResult { added, failed },
    ))
}

pub async fn delete_node(state: Arc<AppState>, req: DeleteNodeRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    let mut edit = ConfigEdit::begin(&state).await;

    let original_len = edit.candidate.nodes.len();
    edit.candidate.nodes.retain(|node_str| {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(node_str) {
            v.get("tag").and_then(|t| t.as_str()) != Some(&req.tag)
        } else {
            true
        }
    });

    if edit.candidate.nodes.len() == original_len {
        return Err(command_error(CommandErrorKind::NotFound, "Node not found"));
    }

    match edit.commit().await {
        Ok(_) => Ok(success_no_data("Node deleted")),
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}
