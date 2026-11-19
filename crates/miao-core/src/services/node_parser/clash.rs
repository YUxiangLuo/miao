use serde_json::Value as JsonValue;
use yaml_serde::Value;

use crate::error::{AppError, AppResult};

use super::protocols::{parse_single_node, ClashProtocol};

const MAX_CLASH_PROXIES: usize = 10_000;

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

    let proxy_list = clash_obj.get("proxies").and_then(Value::as_sequence);
    let proxies = proxy_list.map(Vec::as_slice).unwrap_or_default();
    // Count the original entries, before unsupported/invalid nodes are skipped.
    if proxies.len() > MAX_CLASH_PROXIES {
        return Err(AppError::message("订阅节点数超过 10000 上限"));
    }

    let mut result = ParseResult {
        has_proxy_list: proxy_list.is_some(),
        nodes: vec![],
        errors: vec![],
        total_count: proxies.len(),
    };

    for (idx, node) in proxies.iter().enumerate() {
        let node_type = node
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        // Skip unsupported node types silently rather than recording parse errors.
        let Some(protocol) = ClashProtocol::from_clash_type(node_type) else {
            continue;
        };

        match parse_single_node(protocol, node) {
            Ok((name, outbound)) => result.nodes.push((name, outbound)),
            Err(err) => {
                let fallback_name;
                let name = match node.get("name").and_then(Value::as_str) {
                    Some(name) => name,
                    None => {
                        fallback_name = format!("<index {idx}>");
                        &fallback_name
                    }
                };
                result
                    .errors
                    .push(format!("Node '{name}' (type: {node_type}): {err}"));
            }
        }
    }

    Ok(result)
}
