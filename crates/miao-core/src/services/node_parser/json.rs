/// 节点显示信息结构
#[derive(Debug, Clone)]
pub struct NodeDisplayInfo {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub node_type: String,
    pub sni: Option<String>,
}

/// 解析单个节点 JSON 字符串，返回验证后的 Value 和显示信息
pub fn parse_node_json(node_str: &str) -> Result<(NodeDisplayInfo, serde_json::Value), String> {
    let v: serde_json::Value =
        serde_json::from_str(node_str).map_err(|e| format!("Invalid JSON: {}", e))?;

    let tag = v
        .get("tag")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("Missing or empty tag")?
        .to_string();

    let server = v
        .get("server")
        .and_then(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("Missing or empty server")?
        .to_string();

    let server_port = v
        .get("server_port")
        .and_then(|p| p.as_u64())
        .and_then(|p| {
            if p > 0 && p <= 65535 {
                Some(p as u16)
            } else {
                None
            }
        })
        .ok_or("Invalid or missing port")?;

    let node_type = v
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("unknown")
        .to_string();

    let sni = v
        .get("tls")
        .and_then(|t| t.get("server_name"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let info = NodeDisplayInfo {
        tag,
        server,
        server_port,
        node_type,
        sni,
    };

    Ok((info, v))
}
