use serde_json::{json, Map, Value as JsonValue};
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

/// Protocols recognized from Clash YAML `proxies[].type`.
/// Completely unknown Clash proxy types return `None` and are silently skipped,
/// whereas unsupported wire features *inside* a supported protocol return `Err`
/// so the subscription parser records a diagnostic instead of downgrading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClashProtocol {
    Socks5,
    Hysteria2,
    Anytls,
    Shadowsocks,
    Vmess,
    Vless,
    Trojan,
    Tuic,
}

impl ClashProtocol {
    pub(super) fn from_clash_type(node_type: &str) -> Option<Self> {
        match node_type.to_ascii_lowercase().as_str() {
            "socks5" => Some(Self::Socks5),
            "hysteria2" => Some(Self::Hysteria2),
            "anytls" => Some(Self::Anytls),
            "ss" => Some(Self::Shadowsocks),
            "vmess" => Some(Self::Vmess),
            "vless" => Some(Self::Vless),
            "trojan" => Some(Self::Trojan),
            "tuic" => Some(Self::Tuic),
            _ => None,
        }
    }
}

fn attach_packet_encoding(obj: &mut Map<String, JsonValue>, node: &Value) -> Result<(), String> {
    if let Some(packet_encoding) = get_str(node, "packet-encoding") {
        validate_packet_encoding(packet_encoding)?;
        obj.insert("packet_encoding".to_string(), json!(packet_encoding));
    }
    Ok(())
}

fn attach_v2ray_layers(
    obj: &mut Map<String, JsonValue>,
    node: &Value,
    default_tls: bool,
) -> Result<(), String> {
    if let Some(tls) = build_tls(node, default_tls)? {
        obj.insert("tls".to_string(), tls);
    }
    if let Some(transport) = build_v2ray_transport(node)? {
        obj.insert("transport".to_string(), transport);
    }
    Ok(())
}

/// A Shadowsocks plugin alters the wire protocol; reject non-empty plugin
/// fields rather than silently downgrading to plain Shadowsocks.
fn has_ss_plugin_config(node: &Value) -> bool {
    let has_plugin = match node.get("plugin") {
        None | Some(Value::Null) => false,
        Some(Value::String(plugin)) => !plugin.trim().is_empty(),
        Some(_) => true,
    };
    let has_plugin_opts = match node.get("plugin-opts") {
        None | Some(Value::Null) => false,
        Some(Value::Mapping(opts)) => !opts.is_empty(),
        Some(_) => true,
    };
    has_plugin || has_plugin_opts
}

fn parse_socks5(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
    if get_bool(node, "tls") {
        return Err("SOCKS5 TLS is not supported".to_string());
    }
    // RFC 1929 credentials are byte-oriented: preserve leading/trailing whitespace.
    let username = node
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let password = node
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default();
    crate::validation::Validator::socks5_auth(username, password)?;

    let mut obj = base_outbound("socks", name, server, port);
    obj.insert("version".to_string(), json!("5"));
    if !username.is_empty() {
        obj.insert("username".to_string(), json!(username));
        obj.insert("password".to_string(), json!(password));
    }
    if node.get("udp").and_then(Value::as_bool) == Some(false) {
        obj.insert("network".to_string(), json!("tcp"));
    }
    Ok(obj)
}

fn parse_hysteria2(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
    let password = get_required_str(node, "password")?;
    let mut obj = base_outbound("hysteria2", name, server, port);
    obj.insert("password".to_string(), json!(password));
    obj.insert("tls".to_string(), build_required_tls(node)?);
    if let Some(obfs) = parse_hysteria2_obfs(node)? {
        obj.insert("obfs".to_string(), obfs);
    }
    Ok(obj)
}

fn parse_anytls(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
    let password = get_required_str(node, "password")?;
    let mut obj = base_outbound("anytls", name, server, port);
    obj.insert("password".to_string(), json!(password));
    obj.insert("tls".to_string(), build_required_tls(node)?);
    Ok(obj)
}

fn parse_shadowsocks(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
    if has_ss_plugin_config(node) {
        return Err("unsupported Shadowsocks plugin/plugin-opts".to_string());
    }
    let method = get_required_str(node, "cipher")?;
    let password = get_required_str(node, "password")?;
    let mut obj = base_outbound("shadowsocks", name, server, port);
    obj.insert("method".to_string(), json!(method));
    obj.insert("password".to_string(), json!(password));
    Ok(obj)
}

fn parse_vmess(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
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
    attach_packet_encoding(&mut obj, node)?;
    attach_v2ray_layers(&mut obj, node, false)?;
    Ok(obj)
}

fn parse_vless(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
    if let Some(encryption) = get_str(node, "encryption").filter(|enc| *enc != "none") {
        return Err(format!(
            "unsupported VLESS encryption '{}'; only 'none' is supported",
            encryption
        ));
    }
    let uuid = get_required_str(node, "uuid")?;
    validate_uuid(uuid)?;

    let mut obj = base_outbound("vless", name, server, port);
    obj.insert("uuid".to_string(), json!(uuid));
    if let Some(flow) = get_str(node, "flow") {
        validate_vless_flow(flow)?;
        obj.insert("flow".to_string(), json!(flow));
    }
    attach_packet_encoding(&mut obj, node)?;
    attach_v2ray_layers(&mut obj, node, false)?;
    Ok(obj)
}

fn parse_trojan(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
    let has_ss_opts = node
        .get("ss-opts")
        .and_then(Value::as_mapping)
        .and_then(|opts| map_get_bool(opts, "enabled"))
        .unwrap_or(false);
    if has_ss_opts {
        return Err("unsupported Trojan ss-opts".to_string());
    }
    let password = get_required_str(node, "password")?;
    let mut obj = base_outbound("trojan", name, server, port);
    obj.insert("password".to_string(), json!(password));
    attach_v2ray_layers(&mut obj, node, true)?;
    Ok(obj)
}

fn parse_tuic(
    node: &Value,
    name: &str,
    server: &str,
    port: u16,
) -> Result<Map<String, JsonValue>, String> {
    if get_str(node, "token").is_some() {
        return Err(
            "unsupported TUIC token/v4 format; only TUIC v5 uuid/password is supported".to_string(),
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
    Ok(obj)
}

pub(super) fn parse_single_node(
    protocol: ClashProtocol,
    node: &Value,
) -> Result<(String, JsonValue), String> {
    let name = get_required_str(node, "name")?;
    let server = get_required_str(node, "server")?;
    let port = get_port(node)?;

    let outbound = match protocol {
        ClashProtocol::Socks5 => parse_socks5(node, name, server, port)?,
        ClashProtocol::Hysteria2 => parse_hysteria2(node, name, server, port)?,
        ClashProtocol::Anytls => parse_anytls(node, name, server, port)?,
        ClashProtocol::Shadowsocks => parse_shadowsocks(node, name, server, port)?,
        ClashProtocol::Vmess => parse_vmess(node, name, server, port)?,
        ClashProtocol::Vless => parse_vless(node, name, server, port)?,
        ClashProtocol::Trojan => parse_trojan(node, name, server, port)?,
        ClashProtocol::Tuic => parse_tuic(node, name, server, port)?,
    };

    Ok((name.to_string(), JsonValue::Object(outbound)))
}
