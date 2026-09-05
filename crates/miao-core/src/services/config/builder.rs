use std::collections::HashSet;
use tracing::warn;

use crate::error::{AppError, AppResult};
#[cfg(test)]
use crate::models::node_multiplier;
use crate::models::{Config, NodeMultiplier, NodeSelect, RouteMode};
use crate::state::SkippedRule;

use super::region::node_matches_region;

pub(super) fn make_unique_tag(tag: &str, used: &mut HashSet<String>) -> String {
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
#[cfg(test)]
pub(super) fn filter_rules_with_missing_outbound(
    custom_rules: &[String],
    available_outbounds: &HashSet<String>,
) -> (Vec<String>, Vec<SkippedRule>) {
    let parsed = parse_custom_rules(custom_rules);
    let (kept, skipped) = filter_parsed_rules_with_missing_outbound(parsed, available_outbounds);
    (kept.into_iter().map(|rule| rule.raw).collect(), skipped)
}

#[derive(Clone, Debug)]
struct ParsedCustomRule {
    raw: String,
    value: serde_json::Value,
}

fn parse_custom_rules(custom_rules: &[String]) -> Vec<ParsedCustomRule> {
    custom_rules
        .iter()
        .filter_map(|raw| match serde_json::from_str(raw) {
            Ok(value) => Some(ParsedCustomRule {
                raw: raw.clone(),
                value,
            }),
            Err(err) => {
                warn!(rule = %raw, error = %err, "Failed to parse custom rule");
                None
            }
        })
        .collect()
}

fn filter_parsed_rules_with_missing_outbound(
    custom_rules: Vec<ParsedCustomRule>,
    available_outbounds: &HashSet<String>,
) -> (Vec<ParsedCustomRule>, Vec<SkippedRule>) {
    let mut kept = Vec::with_capacity(custom_rules.len());
    let mut skipped = Vec::new();

    for rule in custom_rules {
        let missing = rule.value.get("outbound").and_then(|value| {
            let outbound = value.as_str()?;
            (!available_outbounds.contains(outbound))
                .then(|| (summarize_rule_matcher(&rule.value), outbound.to_string()))
        });

        match missing {
            Some((summary, outbound)) => {
                warn!(rule = %rule.raw, outbound = %outbound, "Skipping custom rule with missing outbound node");
                skipped.push(SkippedRule {
                    raw: rule.raw,
                    description: format!("{summary} → {outbound}"),
                });
            }
            None => kept.push(rule),
        }
    }

    (kept, skipped)
}

#[cfg(test)]
pub(super) fn build_sing_box_config(
    config: &Config,
    my_names: Vec<String>,
    my_outbounds: Vec<serde_json::Value>,
    final_node_names: Vec<String>,
    final_outbounds: Vec<serde_json::Value>,
) -> AppResult<(serde_json::Value, Vec<SkippedRule>, NodeSelect)> {
    let metadata = my_names
        .iter()
        .chain(&final_node_names)
        .map(|name| NodeMetadata {
            display_name: name.clone(),
            multiplier: node_multiplier(name),
        })
        .collect();
    build_sing_box_config_with_multipliers(
        config,
        my_names,
        my_outbounds,
        final_node_names,
        final_outbounds,
        metadata,
    )
}

/// 地区和倍率来自当前显示元数据，稳定 tag 仅用于引用。
/// 元数据与“手动节点 + 订阅节点”的顺序一一对应。
pub(super) struct NodeMetadata {
    pub display_name: String,
    pub multiplier: Option<NodeMultiplier>,
}

pub(super) fn build_sing_box_config_with_multipliers(
    config: &Config,
    my_names: Vec<String>,
    my_outbounds: Vec<serde_json::Value>,
    final_node_names: Vec<String>,
    final_outbounds: Vec<serde_json::Value>,
    metadata: Vec<NodeMetadata>,
) -> AppResult<(serde_json::Value, Vec<SkippedRule>, NodeSelect)> {
    let total_nodes = my_outbounds.len() + final_outbounds.len();
    if total_nodes == 0 {
        return Err(AppError::NoUsableNodes);
    }

    let (node_names, outbounds) = normalize_outbound_tags(
        my_names.into_iter().chain(final_node_names).collect(),
        my_outbounds.into_iter().chain(final_outbounds).collect(),
    );
    // Stable public tags are identifiers. Region and multiplier belong to
    // the current display metadata, which may change independently of a tag.
    let automatic_members: Vec<String> = node_names
        .iter()
        .zip(&metadata)
        .filter(|(_, meta)| {
            config
                .node_select
                .region()
                .is_some_and(|region| node_matches_region(&meta.display_name, region))
        })
        .filter(|(_, meta)| {
            config
                .max_multiplier
                .is_none_or(|cap| meta.multiplier.is_some_and(|value| value <= cap))
        })
        .map(|(tag, _)| tag.clone())
        .collect();
    let effective_select = if config.node_select.is_manual() || automatic_members.is_empty() {
        NodeSelect::Manual
    } else {
        config.node_select
    };
    let group_names = if effective_select.is_manual() {
        node_names.clone()
    } else {
        automatic_members
    };

    // 规则引用不存在的节点会让 sing-box check 失败;生成时跳过这些规则并留痕告警
    let mut available_outbounds: HashSet<String> = node_names.iter().cloned().collect();
    available_outbounds.insert("proxy".to_string());
    available_outbounds.insert("direct".to_string());
    // Parse once, then use the same typed rules for outbound validation, DNS
    // derivation and route insertion. This removes three subtly different
    // interpretations of the same raw compatibility format.
    let parsed_rules = parse_custom_rules(&config.custom_rules);
    let (custom_rules, skipped_rules) =
        filter_parsed_rules_with_missing_outbound(parsed_rules, &available_outbounds);

    let mut sing_box_config = get_config_template();
    apply_proxy_group(&mut sing_box_config, effective_select, &group_names);
    if let Some(arr) = sing_box_config["outbounds"].as_array_mut() {
        arr.extend(outbounds);
    }

    apply_route_mode(&mut sing_box_config, config.route_mode, &custom_rules);

    Ok((sing_box_config, skipped_rules, effective_select))
}

fn apply_proxy_group(
    sing_box_config: &mut serde_json::Value,
    select: NodeSelect,
    members: &[String],
) {
    let member_values: Vec<serde_json::Value> = members
        .iter()
        .cloned()
        .map(serde_json::Value::String)
        .collect();
    sing_box_config["outbounds"][0] = if select.is_manual() {
        serde_json::json!({
            "type": "selector",
            "tag": "proxy",
            "outbounds": member_values
        })
    } else {
        serde_json::json!({
            "type": "urltest",
            "tag": "proxy",
            "outbounds": member_values,
            // 测速目标与 Clash API 测速端点同口径：sing-box 自 9d32fc9b
            // （Use HTTPS URLTest source）起在 API 层拒绝 http:// URL 并回退到
            // 此 https 默认值，组配置若仍用 http 会造成两套测量口径。
            "url": "https://www.gstatic.com/generate_204",
            "interval": "2m",
            "tolerance": 30,
            // 自动切换不打断已有连接：旧连接留在原节点自然结束，新连接走当前最快
            "interrupt_exist_connections": false
        })
    };
}

/// Route rules and DNS rules are evaluated independently by sing-box. Mirror
/// the subset of custom route matchers whose meaning is stable during a DNS
/// request, so a rule that pins traffic to an outbound also resolves through
/// that outbound.
///
/// Destination ports/IPs and sniffed protocols describe the future data
/// connection, not the DNS request, so copying them would silently broaden or
/// change the rule. Raw compound rules containing any such field are skipped.
const DNS_MIRROR_MATCHERS: &[&str] = &[
    "domain",
    "domain_suffix",
    "domain_keyword",
    "domain_regex",
    "source_ip_cidr",
    "source_ip_is_private",
    "process_name",
    "process_path",
    "process_path_regex",
    "package_name",
    "package_name_regex",
];

fn mirrored_dns_matchers(
    rule: &serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut matchers = serde_json::Map::new();
    for (key, value) in rule {
        if matches!(key.as_str(), "action" | "outbound") {
            continue;
        }
        if !DNS_MIRROR_MATCHERS.contains(&key.as_str()) {
            return None;
        }
        matchers.insert(key.clone(), value.clone());
    }
    (!matchers.is_empty()).then_some(matchers)
}

fn dns_server_for_outbound(
    outbound: &str,
    dns_servers: &mut Vec<serde_json::Value>,
    node_dns_servers: &mut Vec<(String, String)>,
) -> String {
    match outbound {
        "direct" => "local".to_string(),
        "proxy" => "cfdns".to_string(),
        node => {
            if let Some((_, server)) = node_dns_servers.iter().find(|(name, _)| name == node) {
                return server.clone();
            }

            let server = format!("custom-node-dns-{}", node_dns_servers.len() + 1);
            dns_servers.push(serde_json::json!({
                "type": "https",
                "tag": server,
                "server": "1.1.1.1",
                "detour": node
            }));
            node_dns_servers.push((node.to_string(), server.clone()));
            server
        }
    }
}

fn derive_custom_dns_rules(
    custom_rules: &[ParsedCustomRule],
    dns_servers: &mut Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    let mut dns_rules = Vec::new();
    let mut node_dns_servers = Vec::new();

    for parsed_rule in custom_rules {
        let Some(rule) = parsed_rule.value.as_object() else {
            continue;
        };
        if rule
            .get("action")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|action| action != "route")
        {
            continue;
        }
        let Some(outbound) = rule.get("outbound").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(mut dns_rule) = mirrored_dns_matchers(rule) else {
            continue;
        };

        let server = dns_server_for_outbound(outbound, dns_servers, &mut node_dns_servers);
        dns_rule.insert("action".to_string(), serde_json::json!("route"));
        dns_rule.insert("server".to_string(), serde_json::json!(server));
        dns_rules.push(serde_json::Value::Object(dns_rule));
    }

    dns_rules
}

fn apply_dns_route_mode(
    sing_box_config: &mut serde_json::Value,
    route_mode: RouteMode,
    custom_rules: &[ParsedCustomRule],
) {
    let Some(dns_servers) = sing_box_config["dns"]["servers"].as_array_mut() else {
        return;
    };
    let custom_dns_rules = derive_custom_dns_rules(custom_rules, dns_servers);
    let Some(dns_rules) = sing_box_config["dns"]["rules"].as_array_mut() else {
        return;
    };

    if route_mode == RouteMode::Global {
        // Global mode removes only the built-in China split. Custom routing is
        // still active, so its derived DNS policy must remain active too.
        dns_rules.clear();
    }
    dns_rules.splice(0..0, custom_dns_rules);
}

fn apply_route_mode(
    sing_box_config: &mut serde_json::Value,
    route_mode: RouteMode,
    custom_rules: &[ParsedCustomRule],
) {
    if let Some(rules) = sing_box_config["route"]["rules"].as_array_mut() {
        // 两种模式下自定义规则都优先生效
        let insertions = custom_rules.iter().map(|rule| rule.value.clone());
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

    apply_dns_route_mode(sing_box_config, route_mode, custom_rules);
}

pub(super) fn tun_inbound() -> serde_json::Value {
    let mut inbound = serde_json::json!({
        "type": "tun",
        "tag": "tun-in",
        "interface_name": "sing-tun",
        "address": ["172.18.0.1/30"],
        "mtu": 9000,
        "auto_route": true,
        "strict_route": true
    });
    if cfg!(target_os = "linux") {
        inbound["auto_redirect"] = serde_json::json!(true);
    }
    inbound
}

fn get_config_template() -> serde_json::Value {
    serde_json::json!({
        "log": {"disabled": false, "timestamp": true, "level": "info"},
        "experimental": {
            "clash_api": {"external_controller": crate::services::singbox::CLASH_API_HOST},
            "cache_file": {
                "enabled": true,
                "path": "cache.db",
                "store_dns": true
            }
        },
        "dns": {
            "final": "cfdns",
            "strategy": "ipv4_only",
            "reverse_mapping": true,
            "disable_cache": false,
            "cache_capacity": 4096,
            "optimistic": {"enabled": true, "timeout": "8h"},
            "servers": [
                {"type": "https", "tag": "cfdns", "server": "1.1.1.1", "detour": "proxy"},
                {"tag": "local", "type": "udp", "server": "223.5.5.5"}
            ],
            "rules": [
                {"rule_set": ["chinasite"], "action": "route", "server": "local"}
            ]
        },
        "inbounds": [tun_inbound()],
        "outbounds": [
            {"type": "selector", "tag": "proxy", "outbounds": []},
            {"type": "direct", "tag": "direct"}
        ],
        "route": {
            "final": "proxy",
            "auto_detect_interface": true,
            "default_domain_resolver": "local",
            // 无条件启用进程搜索：面板「按进程」视图与 Clash API 的 processPath 依赖它；
            // 否则仅当存在 process 类规则时才收集，无进程规则的用户面板会拿不到数据。
            // 开销为每条新连接一次进程查找，可忽略。
            "find_process": true,
            "rules": [
                {"action": "sniff"},
                {"protocol": "dns", "action": "hijack-dns"},
                {"ip_is_private": true, "action": "route", "outbound": "direct"},
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
