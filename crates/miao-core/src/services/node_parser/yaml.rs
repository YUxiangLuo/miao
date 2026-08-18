use serde_json::{json, Map, Value as JsonValue};
use yaml_serde::{Mapping, Value};

pub(super) fn get_str<'a>(node: &'a Value, key: &str) -> Option<&'a str> {
    node.get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn get_str_any<'a>(node: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| get_str(node, key))
}

pub(super) fn get_required_str<'a>(node: &'a Value, key: &str) -> Result<&'a str, String> {
    get_str(node, key).ok_or_else(|| format!("missing required field '{}'", key))
}

pub(super) fn get_bool_opt(node: &Value, key: &str) -> Option<bool> {
    node.get(key).and_then(|value| value.as_bool())
}

pub(super) fn get_bool(node: &Value, key: &str) -> bool {
    get_bool_opt(node, key).unwrap_or(false)
}

pub(super) fn get_u64_any(node: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        node.get(key).and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
        })
    })
}

pub(super) fn get_port(node: &Value) -> Result<u16, String> {
    let port = get_u64_any(node, &["port"]).ok_or("missing required field 'port'")?;
    if port == 0 || port > 65535 {
        return Err("invalid port".to_string());
    }
    Ok(port as u16)
}

pub(super) fn map_get_str<'a>(map: &'a Mapping, key: &str) -> Option<&'a str> {
    map.get(Value::String(key.to_string()))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn map_get_bool(map: &Mapping, key: &str) -> Option<bool> {
    map.get(Value::String(key.to_string()))
        .and_then(|value| value.as_bool())
}

pub(super) fn map_get_value<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

pub(super) fn yaml_to_json(value: &Value) -> Option<JsonValue> {
    serde_json::to_value(value).ok()
}

pub(super) fn string_list(value: &Value) -> Vec<String> {
    if let Some(items) = value.as_sequence() {
        return items
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect();
    }

    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}

pub(super) fn first_string(value: &Value) -> Option<String> {
    string_list(value).into_iter().next()
}

pub(super) fn base_outbound(
    typ: &str,
    name: &str,
    server: &str,
    port: u16,
) -> Map<String, JsonValue> {
    let mut obj = Map::new();
    obj.insert("type".to_string(), json!(typ));
    obj.insert("tag".to_string(), json!(name));
    obj.insert("server".to_string(), json!(server));
    obj.insert("server_port".to_string(), json!(port));
    obj
}

pub(super) fn insert_optional_string(
    obj: &mut Map<String, JsonValue>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        obj.insert(key.to_string(), json!(value));
    }
}
