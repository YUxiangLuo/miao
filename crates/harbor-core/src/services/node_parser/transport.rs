use serde_json::{json, Map, Value as JsonValue};
use yaml_serde::Value;

use super::validate::validate_client_fingerprint;
use super::yaml::{
    first_string, get_bool, get_bool_opt, get_required_str, get_str, get_str_any,
    insert_optional_string, map_get_str, map_get_value, string_list, yaml_to_json,
};

enum UtlsSetting {
    Disabled,
    Enabled(String),
}

pub(super) fn parse_hysteria2_obfs(node: &Value) -> Result<Option<JsonValue>, String> {
    let Some(obfs_type) = get_str(node, "obfs") else {
        return Ok(None);
    };

    if !matches!(obfs_type, "salamander" | "gecko") {
        return Err(format!("unsupported Hysteria2 obfs type '{}'", obfs_type));
    }

    let password = get_required_str(node, "obfs-password")?;

    Ok(Some(json!({
        "type": obfs_type,
        "password": password
    })))
}

fn parse_alpn(node: &Value) -> Vec<String> {
    node.get("alpn").map(string_list).unwrap_or_default()
}

fn has_reality_public_key(node: &Value) -> bool {
    node.get("reality-opts")
        .and_then(Value::as_mapping)
        .and_then(|opts| map_get_str(opts, "public-key"))
        .is_some()
}

fn has_tls_hints(node: &Value) -> bool {
    get_str_any(node, &["sni", "servername"]).is_some()
        || !parse_alpn(node).is_empty()
        || get_bool(node, "skip-cert-verify")
        || get_str(node, "client-fingerprint").is_some()
        || has_reality_public_key(node)
}

fn base_tls_object(node: &Value) -> Map<String, JsonValue> {
    let mut tls = Map::new();
    tls.insert("enabled".to_string(), json!(true));
    tls.insert(
        "insecure".to_string(),
        json!(get_bool(node, "skip-cert-verify")),
    );
    insert_optional_string(
        &mut tls,
        "server_name",
        get_str_any(node, &["sni", "servername"]),
    );
    tls
}

fn parse_utls_setting(node: &Value) -> Result<UtlsSetting, String> {
    let Some(raw_fingerprint) = get_str(node, "client-fingerprint") else {
        return Ok(UtlsSetting::Disabled);
    };
    let fingerprint = raw_fingerprint.to_ascii_lowercase();
    if fingerprint == "none" {
        return Ok(UtlsSetting::Disabled);
    }
    validate_client_fingerprint(&fingerprint)?;
    Ok(UtlsSetting::Enabled(fingerprint))
}

fn build_reality_opts(node: &Value, utls: &UtlsSetting) -> Result<Option<JsonValue>, String> {
    let Some(reality_opts) = node.get("reality-opts").and_then(Value::as_mapping) else {
        return Ok(None);
    };
    if !matches!(utls, UtlsSetting::Enabled(_)) {
        return Err("Reality requires client-fingerprint/uTLS".to_string());
    }
    let public_key = map_get_str(reality_opts, "public-key")
        .ok_or("missing required Reality field 'public-key'")?;
    let mut reality = Map::new();
    reality.insert("enabled".to_string(), json!(true));
    reality.insert("public_key".to_string(), json!(public_key));
    insert_optional_string(
        &mut reality,
        "short_id",
        map_get_str(reality_opts, "short-id"),
    );
    Ok(Some(JsonValue::Object(reality)))
}

pub(super) fn build_tls(node: &Value, default_enabled: bool) -> Result<Option<JsonValue>, String> {
    let enabled = get_bool_opt(node, "tls").unwrap_or(default_enabled || has_tls_hints(node));
    if !enabled {
        return Ok(None);
    }

    let mut tls = base_tls_object(node);

    let alpn = parse_alpn(node);
    if !alpn.is_empty() {
        tls.insert("alpn".to_string(), json!(alpn));
    }

    let utls = parse_utls_setting(node)?;
    if let UtlsSetting::Enabled(ref fingerprint) = utls {
        tls.insert(
            "utls".to_string(),
            json!({
                "enabled": true,
                "fingerprint": fingerprint
            }),
        );
    }

    if let Some(reality) = build_reality_opts(node, &utls)? {
        tls.insert("reality".to_string(), reality);
    }

    if get_bool(node, "disable-sni") {
        tls.insert("disable_sni".to_string(), json!(true));
    }

    Ok(Some(JsonValue::Object(tls)))
}

/// Protocols whose wire format mandates TLS (Hysteria2, AnyTLS, TUIC) always
/// emit an enabled `tls` block even when a subscription explicitly sets `tls: false`.
pub(super) fn build_required_tls(node: &Value) -> Result<JsonValue, String> {
    if let Some(tls) = build_tls(node, true)? {
        return Ok(tls);
    }
    Ok(JsonValue::Object(base_tls_object(node)))
}

fn build_ws_transport(node: &Value) -> JsonValue {
    let opts = node.get("ws-opts").and_then(Value::as_mapping);
    let mut transport = Map::new();
    transport.insert("type".to_string(), json!("ws"));
    insert_optional_string(
        &mut transport,
        "path",
        opts.and_then(|opts| map_get_str(opts, "path")),
    );
    if let Some(headers) = opts
        .and_then(|opts| map_get_value(opts, "headers"))
        .and_then(yaml_to_json)
    {
        transport.insert("headers".to_string(), headers);
    }
    JsonValue::Object(transport)
}

fn build_grpc_transport(node: &Value) -> JsonValue {
    let opts = node.get("grpc-opts").and_then(Value::as_mapping);
    let mut transport = Map::new();
    transport.insert("type".to_string(), json!("grpc"));
    insert_optional_string(
        &mut transport,
        "service_name",
        opts.and_then(|opts| map_get_str(opts, "grpc-service-name")),
    );
    JsonValue::Object(transport)
}

fn build_http_transport(node: &Value, opts_key: &str) -> JsonValue {
    let opts = node.get(opts_key).and_then(Value::as_mapping);
    let mut transport = Map::new();
    transport.insert("type".to_string(), json!("http"));
    insert_optional_string(
        &mut transport,
        "method",
        opts.and_then(|opts| map_get_str(opts, "method")),
    );
    if let Some(path) = opts
        .and_then(|opts| map_get_value(opts, "path"))
        .and_then(first_string)
    {
        transport.insert("path".to_string(), json!(path));
    }
    if let Some(hosts) = opts
        .and_then(|opts| map_get_value(opts, "host"))
        .map(string_list)
        .filter(|hosts| !hosts.is_empty())
    {
        transport.insert("host".to_string(), json!(hosts));
    }
    if let Some(headers) = opts
        .and_then(|opts| map_get_value(opts, "headers"))
        .and_then(yaml_to_json)
    {
        transport.insert("headers".to_string(), headers);
    }
    JsonValue::Object(transport)
}

pub(super) fn build_v2ray_transport(node: &Value) -> Result<Option<JsonValue>, String> {
    let network = get_str(node, "network")
        .unwrap_or("tcp")
        .to_ascii_lowercase();

    match network.as_str() {
        "" | "tcp" => Ok(None),
        "ws" => Ok(Some(build_ws_transport(node))),
        "grpc" => Ok(Some(build_grpc_transport(node))),
        "http" => Ok(Some(build_http_transport(node, "http-opts"))),
        "h2" => Ok(Some(build_http_transport(node, "h2-opts"))),
        "xhttp" => Err("unsupported transport network 'xhttp'".to_string()),
        other => Err(format!("unsupported transport network '{}'", other)),
    }
}
