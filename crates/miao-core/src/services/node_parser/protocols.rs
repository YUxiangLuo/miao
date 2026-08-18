use serde_json::{json, Value as JsonValue};
use yaml_serde::Value;

use super::transport::{
    build_required_tls, build_tls, build_v2ray_transport, parse_hysteria2_obfs,
};
use super::validate::{
    validate_packet_encoding, validate_tuic_congestion_control, validate_tuic_udp_relay_mode,
    validate_uuid, validate_vless_flow, validate_vmess_security,
};
use super::yaml::{
    base_outbound, get_bool, get_port, get_required_str, get_str, get_u64_any, map_get_bool,
};

pub(super) fn is_supported_node_type(node_type: &str) -> bool {
    matches!(
        node_type,
        "hysteria2" | "anytls" | "ss" | "vmess" | "vless" | "trojan" | "tuic"
    )
}
pub(super) fn parse_single_node(node: &Value) -> Result<(String, JsonValue), String> {
    let typ = get_required_str(node, "type")?.to_ascii_lowercase();
    let name = get_required_str(node, "name")?;

    let server = get_required_str(node, "server")?;
    let port = get_port(node)?;

    let outbound = match typ.as_str() {
        "hysteria2" => {
            let password = get_required_str(node, "password")?;
            let mut obj = base_outbound("hysteria2", name, server, port);
            obj.insert("password".to_string(), json!(password));
            obj.insert("tls".to_string(), build_required_tls(node)?);
            if let Some(obfs) = parse_hysteria2_obfs(node)? {
                obj.insert("obfs".to_string(), obfs);
            }
            JsonValue::Object(obj)
        }
        "anytls" => {
            let password = get_required_str(node, "password")?;
            let mut obj = base_outbound("anytls", name, server, port);
            obj.insert("password".to_string(), json!(password));
            obj.insert("tls".to_string(), build_required_tls(node)?);
            JsonValue::Object(obj)
        }
        "ss" => {
            let method = get_required_str(node, "cipher")?;
            let password = get_required_str(node, "password")?;
            let mut obj = base_outbound("shadowsocks", name, server, port);
            obj.insert("method".to_string(), json!(method));
            obj.insert("password".to_string(), json!(password));
            JsonValue::Object(obj)
        }
        "vmess" => {
            let uuid = get_required_str(node, "uuid")?;
            validate_uuid(uuid)?;
            let security = get_str(node, "cipher").unwrap_or("auto");
            validate_vmess_security(security)?;
            let mut obj = base_outbound("vmess", name, server, port);
            obj.insert("uuid".to_string(), json!(uuid));
            obj.insert("security".to_string(), json!(security));
            obj.insert(
                "alter_id".to_string(),
                json!(get_u64_any(node, &["alterId", "alter-id"]).unwrap_or(0)),
            );
            if let Some(packet_encoding) = get_str(node, "packet-encoding") {
                validate_packet_encoding(packet_encoding)?;
                obj.insert("packet_encoding".to_string(), json!(packet_encoding));
            }
            if let Some(tls) = build_tls(node, false)? {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_v2ray_transport(node)? {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "vless" => {
            if let Some(encryption) = get_str(node, "encryption") {
                if encryption != "none" {
                    return Err(format!(
                        "unsupported VLESS encryption '{}'; only 'none' is supported",
                        encryption
                    ));
                }
            }
            let uuid = get_required_str(node, "uuid")?;
            validate_uuid(uuid)?;
            let mut obj = base_outbound("vless", name, server, port);
            obj.insert("uuid".to_string(), json!(uuid));
            if let Some(flow) = get_str(node, "flow") {
                validate_vless_flow(flow)?;
                obj.insert("flow".to_string(), json!(flow));
            }
            if let Some(packet_encoding) = get_str(node, "packet-encoding") {
                validate_packet_encoding(packet_encoding)?;
                obj.insert("packet_encoding".to_string(), json!(packet_encoding));
            }
            if let Some(tls) = build_tls(node, false)? {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_v2ray_transport(node)? {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "trojan" => {
            if node
                .get("ss-opts")
                .and_then(|value| value.as_mapping())
                .and_then(|opts| map_get_bool(opts, "enabled"))
                .unwrap_or(false)
            {
                return Err("unsupported Trojan ss-opts".to_string());
            }
            let password = get_required_str(node, "password")?;
            let mut obj = base_outbound("trojan", name, server, port);
            obj.insert("password".to_string(), json!(password));
            if let Some(tls) = build_tls(node, true)? {
                obj.insert("tls".to_string(), tls);
            }
            if let Some(transport) = build_v2ray_transport(node)? {
                obj.insert("transport".to_string(), transport);
            }
            JsonValue::Object(obj)
        }
        "tuic" => {
            if get_str(node, "token").is_some() {
                return Err(
                    "unsupported TUIC token/v4 format; only TUIC v5 uuid/password is supported"
                        .to_string(),
                );
            }
            let uuid = get_required_str(node, "uuid")?;
            validate_uuid(uuid)?;
            let password = get_required_str(node, "password")?;
            let mut obj = base_outbound("tuic", name, server, port);
            obj.insert("uuid".to_string(), json!(uuid));
            obj.insert("password".to_string(), json!(password));
            if let Some(congestion_control) = get_str(node, "congestion-controller") {
                validate_tuic_congestion_control(congestion_control)?;
                obj.insert("congestion_control".to_string(), json!(congestion_control));
            }
            if let Some(udp_relay_mode) = get_str(node, "udp-relay-mode") {
                validate_tuic_udp_relay_mode(udp_relay_mode)?;
                obj.insert("udp_relay_mode".to_string(), json!(udp_relay_mode));
            }
            if get_bool(node, "reduce-rtt") {
                obj.insert("zero_rtt_handshake".to_string(), json!(true));
            }
            obj.insert("tls".to_string(), build_required_tls(node)?);
            JsonValue::Object(obj)
        }
        _ => return Err(format!("unsupported node type '{}'", typ)),
    };

    Ok((name.to_string(), outbound))
}
