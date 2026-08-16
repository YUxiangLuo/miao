use std::collections::HashSet;
use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::models::{Config, RouteMode};
use crate::state::SkippedRule;

fn make_unique_tag(tag: &str, used: &mut HashSet<String>) -> String {
    let base = if tag.trim().is_empty() { "node" } else { tag };
    if used.insert(base.to_string()) {
        return base.to_string();
    }

    for index in 2.. {
        let candidate = format!("{base} ({index})");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded duplicate tag search should always find a value")
}

fn normalize_outbound_tags(
    node_names: Vec<String>,
    outbounds: Vec<serde_json::Value>,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let names_len = node_names.len();
    let mut used = HashSet::new();
    // Built-in outbounds from the template already reserve these tags.
    used.insert("proxy".to_string());
    used.insert("direct".to_string());
    let mut unique_names = Vec::with_capacity(outbounds.len());
    let mut unique_outbounds = Vec::with_capacity(outbounds.len());

    for (idx, mut outbound) in outbounds.into_iter().enumerate() {
        let original_name = node_names
            .get(idx)
            .cloned()
            .or_else(|| {
                outbound
                    .get("tag")
                    .and_then(|tag| tag.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("node-{}", idx + 1));
        let unique_name = make_unique_tag(&original_name, &mut used);

        if unique_name != original_name {
            warn!(
                from = %original_name,
                to = %unique_name,
                "Renamed duplicate outbound tag to avoid sing-box conflict"
            );
        }

        if let Some(obj) = outbound.as_object_mut() {
            obj.insert(
                "tag".to_string(),
                serde_json::Value::String(unique_name.clone()),
            );
        } else {
            warn!(tag = %unique_name, "Outbound is not a JSON object; cannot set tag");
        }

        unique_names.push(unique_name);
        unique_outbounds.push(outbound);
    }

    if names_len != unique_outbounds.len() {
        warn!(
            names = names_len,
            outbounds = unique_outbounds.len(),
            "Outbound name count did not match outbound config count"
        );
    }

    (unique_names, unique_outbounds)
}

/// 提取规则的匹配条件摘要,用于告警文案
fn summarize_rule_matcher(rule: &serde_json::Value) -> String {
    const MATCHER_FIELDS: &[&str] = &[
        "domain_suffix",
        "domain",
        "domain_keyword",
        "ip_cidr",
        "source_ip_cidr",
        "port",
        "port_range",
        "protocol",
        "process_name",
        "process_path",
        "rule_set",
    ];
    for field in MATCHER_FIELDS {
        if let Some(value) = rule.get(*field) {
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Array(items) => items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                other => other.to_string(),
            };
            return format!("{field}={value_str}");
        }
    }
    "自定义规则".to_string()
}

/// 跳过引用不存在出口节点的自定义规则,返回(保留的规则, 失效规则的告警描述)。
/// 节点可能因订阅刷新改名或消失;跳过而非写入,避免 sing-box check 失败卡死所有配置变更。
pub(super) fn filter_rules_with_missing_outbound(
    custom_rules: &[String],
    available_outbounds: &HashSet<String>,
) -> (Vec<String>, Vec<SkippedRule>) {
    let mut kept = Vec::with_capacity(custom_rules.len());
    let mut skipped = Vec::new();

    for rule_str in custom_rules {
        let missing = serde_json::from_str::<serde_json::Value>(rule_str)
            .ok()
            .and_then(|rule| {
                let outbound = rule.get("outbound")?.as_str()?;
                if available_outbounds.contains(outbound) {
                    None
                } else {
                    Some((summarize_rule_matcher(&rule), outbound.to_string()))
                }
            });

        match missing {
            Some((summary, outbound)) => {
                warn!(rule = %rule_str, outbound = %outbound, "Skipping custom rule with missing outbound node");
                skipped.push(SkippedRule {
                    raw: rule_str.clone(),
                    description: format!("{summary} → {outbound}"),
                });
            }
            // 无法解析的规则保留原有行为:交给 parse_custom_rules 跳过并告警
            None => kept.push(rule_str.clone()),
        }
    }

    (kept, skipped)
}

pub(super) fn build_sing_box_config(
    config: &Config,
    my_names: Vec<String>,
    my_outbounds: Vec<serde_json::Value>,
    final_node_names: Vec<String>,
    final_outbounds: Vec<serde_json::Value>,
) -> AppResult<(serde_json::Value, Vec<SkippedRule>)> {
    let total_nodes = my_outbounds.len() + final_outbounds.len();
    if total_nodes == 0 {
        return Err(AppError::NoUsableNodes);
    }

    let (node_names, outbounds) = normalize_outbound_tags(
        my_names.into_iter().chain(final_node_names).collect(),
        my_outbounds.into_iter().chain(final_outbounds).collect(),
    );

    // 规则引用不存在的节点会让 sing-box check 失败;生成时跳过这些规则并留痕告警
    let mut available_outbounds: HashSet<String> = node_names.iter().cloned().collect();
    available_outbounds.insert("proxy".to_string());
    available_outbounds.insert("direct".to_string());
    let (custom_rules, skipped_rules) =
        filter_rules_with_missing_outbound(&config.custom_rules, &available_outbounds);

    let mut sing_box_config = get_config_template();
    if let Some(selector_outbounds) = sing_box_config["outbounds"][0].get_mut("outbounds") {
        if let Some(arr) = selector_outbounds.as_array_mut() {
            arr.extend(node_names.into_iter().map(serde_json::Value::String));
        }
    }
    if let Some(arr) = sing_box_config["outbounds"].as_array_mut() {
        arr.extend(outbounds);
    }

    apply_route_mode(
        &mut sing_box_config,
        config.route_mode,
        &custom_rules,
        config.adblock,
    );

    Ok((sing_box_config, skipped_rules))
}

fn parse_custom_rules(custom_rules: &[String]) -> Vec<serde_json::Value> {
    let mut parsed = Vec::new();
    for rule_str in custom_rules {
        if let Ok(rule_json) = serde_json::from_str::<serde_json::Value>(rule_str) {
            parsed.push(rule_json);
        } else {
            warn!("Failed to parse custom rule: {}", rule_str);
        }
    }
    parsed
}

fn apply_route_mode(
    sing_box_config: &mut serde_json::Value,
    route_mode: RouteMode,
    custom_rules: &[String],
    adblock: bool,
) {
    // 广告规则集(REIJI007/AdBlock_Rule_For_Sing-box 编译的本地 srs)仅在使用时挂载
    if adblock {
        if let Some(rule_sets) = sing_box_config["route"]["rule_set"].as_array_mut() {
            rule_sets.insert(
                0,
                serde_json::json!({
                    "type": "local",
                    "tag": "adblock",
                    "format": "binary",
                    "path": "./adblock_reject.srs"
                }),
            );
        }
    }

    if let Some(rules) = sing_box_config["route"]["rules"].as_array_mut() {
        // 两种模式下自定义规则都优先生效;广告拦截排在其后,用户可放行误拦域名
        let mut insertions = parse_custom_rules(custom_rules);
        if adblock {
            insertions.push(serde_json::json!({"rule_set": ["adblock"], "action": "reject"}));
        }
        match route_mode {
            RouteMode::Rule => {
                // Preserve the mandatory pre-routing actions, then let user rules take
                // precedence over the built-in direct/proxy split rules.
                let insertion_index = rules.len().min(2);
                rules.splice(insertion_index..insertion_index, insertions);
            }
            RouteMode::Global => {
                // 全局模式只裁掉内置分流规则,自定义规则(如内网直连)仍然生效
                rules.truncate(2);
                rules.extend(insertions);
            }
        }
    }

    if route_mode == RouteMode::Global {
        if let Some(dns_rules) = sing_box_config["dns"]["rules"].as_array_mut() {
            dns_rules.clear();
        }
    }
}

fn get_config_template() -> serde_json::Value {
    serde_json::json!({
        "log": {"disabled": false, "timestamp": true, "level": "info"},
        "experimental": {
            "clash_api": {"external_controller": "127.0.0.1:6262"},
            "cache_file": {
                "enabled": true,
                "path": "cache.db",
                "store_dns": true
            }
        },
        "dns": {
            "final": "cfdns",
            "strategy": "ipv4_only",
            "disable_cache": false,
            "cache_capacity": 4096,
            "optimistic": {"enabled": true, "timeout": "8h"},
            "servers": [
                {"type": "https", "tag": "cfdns", "server": "1.1.1.1", "detour": "proxy"},
                {"tag": "local", "type": "udp", "server": "223.5.5.5"}
            ],
            "rules": [
                // hdslb.com 是 B 站视频 CDN:强制走国内 DNS,避免解析结果被代理带偏导致视频卡顿
                {"domain_suffix": ["hdslb.com"], "action": "route", "server": "local"},
                {"rule_set": ["chinasite"], "action": "route", "server": "local"}
            ]
        },
        "inbounds": [
            {"type": "tun", "tag": "tun-in", "interface_name": "sing-tun", "address": ["172.18.0.1/30"], "mtu": 9000, "auto_route": true, "strict_route": true, "auto_redirect": true}
        ],
        "outbounds": [
            {"type": "selector", "tag": "proxy", "outbounds": []},
            {"type": "direct", "tag": "direct"}
        ],
        "route": {
            "final": "proxy",
            "auto_detect_interface": true,
            "default_domain_resolver": "local",
            "rules": [
                {"action": "sniff"},
                {"protocol": "dns", "action": "hijack-dns"},
                {"ip_is_private": true, "action": "route", "outbound": "direct"},
                // 与上面的 DNS 规则配套:B 站视频 CDN 流量强制直连
                {"domain_suffix": ["hdslb.com"], "action": "route", "outbound": "direct"},
                {"rule_set": ["chinasite"], "action": "route", "outbound": "direct"},
                {"rule_set": ["chinaip"], "action": "route", "outbound": "direct"}
            ],
            "rule_set": [
                {"type": "local", "tag": "chinasite", "format": "binary", "path": "./chinasite.srs"},
                {"type": "local", "tag": "chinaip", "format": "binary", "path": "./chinaip.srs"}
            ]
        }
    })
}
