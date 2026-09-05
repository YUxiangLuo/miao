use serde_json::Value as JsonValue;
use yaml_serde::Value;

use crate::error::{AppError, AppResult};

use super::protocols::{is_supported_node_type, parse_single_node};

/// 节点解析结果，包含有效节点和错误记录
#[derive(Debug)]
pub struct ParseResult {
    pub has_proxy_list: bool,
    pub nodes: Vec<(String, JsonValue)>, // (name, outbound_json)
    pub errors: Vec<String>,             // 记录解析失败的节点及原因
    pub total_count: usize,              // YAML 中 proxies 列表的原始总数
}

/// 从 Clash 配置中解析节点，跳过无效节点并记录错误
pub fn parse_clash_proxies(clash_yaml: &str) -> AppResult<ParseResult> {
    let clash_obj: Value = yaml_serde::from_str(clash_yaml)
        .map_err(|e| AppError::context("Failed to parse subscription YAML", e))?;

    let proxy_list = clash_obj.get("proxies").and_then(|p| p.as_sequence());
    let proxies = proxy_list.map(Vec::as_slice).unwrap_or_default();

    let mut result = ParseResult {
        has_proxy_list: proxy_list.is_some(),
        nodes: vec![],
        errors: vec![],
        total_count: proxies.len(),
    };

    for (idx, node) in proxies.iter().enumerate() {
        let node_type = node
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("unknown");
        let normalized_type = node_type.to_ascii_lowercase();

        // Skip unsupported node types silently
        if !is_supported_node_type(&normalized_type) {
            continue;
        }

        match parse_single_node(node) {
            Ok((name, outbound)) => result.nodes.push((name, outbound)),
            Err(err) => {
                let name = node
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("<index {}>", idx));
                result
                    .errors
                    .push(format!("Node '{}' (type: {}): {}", name, node_type, err));
            }
        }
    }

    Ok(result)
}
