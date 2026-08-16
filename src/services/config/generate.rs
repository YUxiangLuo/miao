use futures::{stream, StreamExt};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{error, info, warn};

use crate::error::AppResult;
use crate::models::{Config, SubStatus};
use crate::services::singbox::get_sing_box_home;
use crate::services::subscription::fetch_sub;
use crate::state::AppState;

use super::builder::build_sing_box_config;
use super::persist::write_file_atomic;

const MAX_CONCURRENT_SUBS: usize = 5;
/// Returns `true` if at least one subscription node was fetched successfully.
pub async fn gen_config(config: &Config, state: &Arc<AppState>) -> AppResult<bool> {
    let (my_outbounds, my_names) = collect_manual_outbounds(config);
    let mut final_outbounds: Vec<serde_json::Value> = vec![];
    let mut final_node_names: Vec<String> = vec![];

    {
        let mut status_map = state.sub_status.lock().await;
        status_map.retain(|url, _| config.subs.contains(url));
    }

    let sub_futures: Vec<_> = config
        .subs
        .iter()
        .map(|sub| {
            let sub = sub.clone();
            let client = state.http_client.clone();
            async move {
                info!(url = %sub, "Fetching subscription");
                let result =
                    tokio::time::timeout(Duration::from_secs(30), fetch_sub(&sub, &client)).await;

                match result {
                    Ok(Ok(fetch_result)) => {
                        let valid_count = fetch_result.node_names.len();
                        let total_count = fetch_result.total_count;
                        let error_count = fetch_result.parse_errors.len();

                        if error_count > 0 {
                            warn!(
                                url = %sub,
                                valid = valid_count,
                                total = total_count,
                                errors = error_count,
                                "Partial fetch: some nodes failed to parse"
                            );
                        } else {
                            info!(
                                url = %sub,
                                nodes = valid_count,
                                "Subscription fetched successfully"
                            );
                        }

                        (sub.clone(), Ok(fetch_result))
                    }
                    Ok(Err(e)) => {
                        error!(url = %sub, error = %e, "Failed to fetch subscription");
                        (sub.clone(), Err(e.to_string()))
                    }
                    Err(_) => {
                        error!(url = %sub, timeout_secs = 30, "Subscription fetch timed out");
                        (sub.clone(), Err("Request timeout".to_string()))
                    }
                }
            }
        })
        .collect();

    // 使用 buffer_unordered 限制并发数，避免同时发起过多请求
    let mut results: Vec<_> = stream::iter(sub_futures)
        .buffer_unordered(MAX_CONCURRENT_SUBS)
        .collect()
        .await;

    // 按原始顺序排序结果
    let subs_order: Vec<String> = config.subs.clone();
    results.sort_by_key(|(url, _)| {
        subs_order
            .iter()
            .position(|s| s == url)
            .unwrap_or(usize::MAX)
    });

    for (url, result) in results {
        let status = match result {
            Ok(fetch_result) => {
                let count = fetch_result.node_names.len();
                final_node_names.extend(fetch_result.node_names);
                final_outbounds.extend(fetch_result.outbounds);

                let error_info = if !fetch_result.parse_errors.is_empty() {
                    Some(format!(
                        "{} nodes skipped due to parse errors",
                        fetch_result.parse_errors.len()
                    ))
                } else if count == 0 && fetch_result.total_count > 0 {
                    Some("All nodes invalid (missing required fields)".into())
                } else if count == 0 {
                    Some("No nodes found".into())
                } else {
                    None
                };

                SubStatus {
                    url: url.clone(),
                    success: count > 0,
                    node_count: count,
                    error: error_info,
                }
            }
            Err(e) => SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                error: Some(e),
            },
        };
        state.sub_status.lock().await.insert(url, status);
    }

    let has_sub_nodes = !final_node_names.is_empty();

    let (sing_box_config, skipped_rules) = build_sing_box_config(
        config,
        my_names,
        my_outbounds,
        final_node_names,
        final_outbounds,
    )?;

    let sing_box_home = get_sing_box_home();
    let config_output_loc = sing_box_home.join("config.json");
    write_file_atomic(
        &config_output_loc,
        &serde_json::to_string(&sing_box_config)?,
    )
    .await?;

    *state.skipped_rules.lock().await = skipped_rules;

    Ok(has_sub_nodes)
}

pub(super) fn collect_manual_outbounds(config: &Config) -> (Vec<serde_json::Value>, Vec<String>) {
    use crate::services::node_parser::parse_node_json;

    let mut my_outbounds = vec![];
    let mut my_names = vec![];

    for (idx, node_str) in config.nodes.iter().enumerate() {
        // 验证节点并获取解析后的 Value
        match parse_node_json(node_str) {
            Ok((info, outbound)) => {
                my_names.push(info.tag);
                my_outbounds.push(outbound);
            }
            Err(e) => {
                warn!("[collect_manual_outbounds] Skipping node #{}: {}", idx, e);
            }
        }
    }

    (my_outbounds, my_names)
}

/// 从运行时 sing-box 配置中提取用户节点 tag(排除内置 outbound)
pub(super) fn runtime_config_node_tags(config_json: &serde_json::Value) -> Vec<String> {
    config_json["outbounds"]
        .as_array()
        .map(|outbounds| {
            outbounds
                .iter()
                .filter_map(|outbound| outbound["tag"].as_str())
                .filter(|tag| *tag != "proxy" && *tag != "direct")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 自定义规则可选的节点目标:手动节点(配置直读)+ 订阅节点(最近一次生成的运行时配置)。
/// 仅供规则校验给出友好报错,最终正确性由 sing-box check 保证。
pub async fn known_rule_targets(config: &Config) -> Vec<String> {
    use crate::services::node_parser::parse_node_json;

    let mut tags: Vec<String> = Vec::new();
    for node_str in &config.nodes {
        if let Ok((info, _)) = parse_node_json(node_str) {
            if !tags.contains(&info.tag) {
                tags.push(info.tag);
            }
        }
    }

    let runtime_config_path = get_sing_box_home().join("config.json");
    if let Ok(content) = tokio::fs::read_to_string(&runtime_config_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            for tag in runtime_config_node_tags(&json) {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
    }

    tags
}
