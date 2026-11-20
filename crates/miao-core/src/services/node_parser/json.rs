use serde_json::Value as JsonValue;
use std::sync::LazyLock;

#[derive(serde::Deserialize)]
struct KernelProfile {
    node_protocols: Vec<String>,
}

static KERNEL_PROFILE: LazyLock<KernelProfile> = LazyLock::new(|| {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/sing-box/source.json"
    )))
    .expect("kernel source manifest must contain node_protocols")
});

/// 节点显示信息结构
#[derive(Debug, Clone)]
pub struct NodeDisplayInfo {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub node_type: String,
    pub sni: Option<String>,
    pub detour: Option<String>,
}

fn parse_valid_port(value: &JsonValue) -> Option<u16> {
    value
        .get("server_port")
        .and_then(JsonValue::as_u64)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|&port| port > 0)
}

/// 解析单个节点 JSON 字符串，返回验证后的 Value 和显示信息
pub fn parse_node_json(node_str: &str) -> Result<(NodeDisplayInfo, JsonValue), String> {
    let v: JsonValue =
        serde_json::from_str(node_str).map_err(|e| format!("Invalid JSON: {}", e))?;

    let tag = v
        .get("tag")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("Missing or empty tag")?
        .to_string();

    let server = v
        .get("server")
        .and_then(JsonValue::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("Missing or empty server")?
        .to_string();

    let server_port = parse_valid_port(&v).ok_or("Invalid or missing port")?;

    let node_type = v
        .get("type")
        .and_then(JsonValue::as_str)
        .unwrap_or("unknown")
        .to_string();

    if !KERNEL_PROFILE.node_protocols.contains(&node_type) {
        return Err(format!(
            "当前内核不支持节点协议 {}，支持的协议：{}",
            node_type,
            KERNEL_PROFILE.node_protocols.join(", ")
        ));
    }

    let sni = v
        .get("tls")
        .and_then(|t| t.get("server_name"))
        .and_then(JsonValue::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let detour = v
        .get("detour")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let info = NodeDisplayInfo {
        tag,
        server,
        server_port,
        node_type,
        sni,
        detour,
    };

    Ok((info, v))
}
