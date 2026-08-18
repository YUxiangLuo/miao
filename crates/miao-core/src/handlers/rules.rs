use axum::{extract::State, http::StatusCode, response::Json};
use std::sync::{atomic::Ordering, Arc};

use crate::models::{ApiResponse, DeleteRuleRequest, RuleInfo, RuleRequest};
use crate::responses::{status_error, success, success_no_data, HandlerResult};
use crate::services::config::{apply_config_change, known_rule_targets};
use crate::state::AppState;
use crate::validation::Validator;
use serde_json::{json, Map, Value as JsonValue};

/// 由 UI 表单生成规范的 sing-box 规则 JSON(action 格式)
fn build_rule_json(req: &RuleRequest) -> JsonValue {
    let mut rule = Map::new();
    let value = if req.field == "port" {
        json!(req.value.trim().parse::<u16>().unwrap_or(0))
    } else {
        json!(req.value.trim())
    };
    rule.insert(req.field.clone(), value);
    if req.target == "reject" {
        rule.insert("action".to_string(), json!("reject"));
    } else {
        rule.insert("action".to_string(), json!("route"));
        rule.insert("outbound".to_string(), json!(req.target));
    }
    JsonValue::Object(rule)
}

/// 把存储的 JSON 字符串还原成结构化展示;无法识别的手写规则以 raw 兜底
pub(crate) fn describe_rule(index: usize, raw: &str) -> RuleInfo {
    let parsed = serde_json::from_str::<JsonValue>(raw).ok();
    let mut info = RuleInfo {
        index,
        field: None,
        value: None,
        target: None,
        skipped: false,
        raw: raw.to_string(),
    };

    let Some(JsonValue::Object(map)) = parsed else {
        return info;
    };

    const KNOWN_FIELDS: &[&str] = &[
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
    ];

    // 仅单条件规则做结构化展示:零个或多个已知匹配字段(含两个白名单字段的复合规则)
    // 都回退 raw,避免隐藏条件误导用户
    let matched: Vec<&&str> = KNOWN_FIELDS
        .iter()
        .filter(|field| map.contains_key(**field))
        .collect();
    if matched.len() != 1 {
        return info;
    }
    let field = *matched[0];

    // 出现匹配字段/action/outbound 之外的键,同样回退 raw
    let has_unknown_keys = map
        .keys()
        .any(|key| !KNOWN_FIELDS.contains(&key.as_str()) && key != "action" && key != "outbound");
    if has_unknown_keys {
        return info;
    }

    let value = match &map[field] {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        _ => return info,
    };

    // 新格式 action 优先,兼容旧版 outbound 内联写法
    let target = match map.get("action").and_then(|a| a.as_str()) {
        Some("reject") => Some("reject".to_string()),
        Some("route") => map
            .get("outbound")
            .and_then(|o| o.as_str())
            .map(str::to_string),
        _ => map
            .get("outbound")
            .and_then(|o| o.as_str())
            .map(str::to_string),
    };

    info.field = Some(field.to_string());
    info.value = Some(value);
    info.target = target;
    info
}

pub async fn get_rules(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<RuleInfo>>> {
    let config = state.config.read().await;
    let skipped_rules = state.skipped_rules.lock().await;
    let skipped_raws: std::collections::HashSet<&str> =
        skipped_rules.iter().map(|rule| rule.raw.as_str()).collect();
    let rules: Vec<RuleInfo> = config
        .custom_rules
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let mut info = describe_rule(index, raw);
            info.skipped = skipped_raws.contains(raw.as_str());
            info
        })
        .collect();

    success("Rules loaded", rules)
}

pub async fn add_rule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RuleRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let extra_targets = known_rule_targets(&state.config.read().await.clone()).await;
    Validator::custom_rule(&req, &extra_targets)
        .map_err(|e| status_error(StatusCode::BAD_REQUEST, e))?;

    let rule_json = build_rule_json(&req);
    let rule_str = serde_json::to_string(&rule_json)
        .map_err(|e| status_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();

    if new_config.custom_rules.contains(&rule_str) {
        return Err(status_error(StatusCode::BAD_REQUEST, "规则已存在"));
    }

    new_config.custom_rules.push(rule_str);

    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success_no_data("Rule added")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

pub async fn delete_rule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteRuleRequest>,
) -> HandlerResult {
    if state.initializing.load(Ordering::Relaxed) {
        return Err(status_error(
            StatusCode::CONFLICT,
            "Initialization is still in progress",
        ));
    }

    let _config_update = state.config_update.lock().await;
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();

    let Some(existing) = new_config.custom_rules.get(req.index) else {
        return Err(status_error(StatusCode::NOT_FOUND, "Rule not found"));
    };
    // 列表可能因并发变更(其他标签页/轮询)前移,删除前必须确认条目未被挪动
    if existing != &req.raw {
        return Err(status_error(
            StatusCode::CONFLICT,
            "规则列表已变化,请刷新后重试",
        ));
    }
    new_config.custom_rules.remove(req.index);

    match apply_config_change(&state, &old_config, &new_config).await {
        Ok(_) => Ok(success_no_data("Rule deleted")),
        Err(e) => Err(status_error(StatusCode::INTERNAL_SERVER_ERROR, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_rule_json, describe_rule};
    use crate::models::RuleRequest;
    use serde_json::json;

    fn request(field: &str, value: &str, target: &str) -> RuleRequest {
        RuleRequest {
            field: field.to_string(),
            value: value.to_string(),
            target: target.to_string(),
        }
    }

    #[test]
    fn build_rule_json_routes_proxy_and_direct() {
        let rule = build_rule_json(&request("domain_suffix", "example.com", "proxy"));
        assert_eq!(
            rule,
            json!({"domain_suffix": "example.com", "action": "route", "outbound": "proxy"})
        );
    }

    #[test]
    fn build_rule_json_routes_to_specific_node() {
        let rule = build_rule_json(&request("process_path", "/usr/bin/curl", "香港节点"));
        assert_eq!(
            rule,
            json!({"process_path": "/usr/bin/curl", "action": "route", "outbound": "香港节点"})
        );
    }

    #[test]
    fn build_rule_json_reject_has_no_outbound() {
        let rule = build_rule_json(&request("process_name", "curl", "reject"));
        assert_eq!(rule, json!({"process_name": "curl", "action": "reject"}));
        assert!(rule.get("outbound").is_none());
    }

    #[test]
    fn build_rule_json_stores_port_as_number() {
        let rule = build_rule_json(&request("port", "25", "direct"));
        assert_eq!(
            rule,
            json!({"port": 25, "action": "route", "outbound": "direct"})
        );
    }

    #[test]
    fn describe_rule_reads_canonical_action_format() {
        let info = describe_rule(
            0,
            r#"{"domain_suffix":"example.com","action":"route","outbound":"direct"}"#,
        );
        assert_eq!(info.field.as_deref(), Some("domain_suffix"));
        assert_eq!(info.value.as_deref(), Some("example.com"));
        assert_eq!(info.target.as_deref(), Some("direct"));
    }

    #[test]
    fn describe_rule_reads_reject_action() {
        let info = describe_rule(1, r#"{"process_name":"curl","action":"reject"}"#);
        assert_eq!(info.field.as_deref(), Some("process_name"));
        assert_eq!(info.value.as_deref(), Some("curl"));
        assert_eq!(info.target.as_deref(), Some("reject"));
    }

    #[test]
    fn describe_rule_reads_legacy_outbound_format_and_arrays() {
        let info = describe_rule(
            2,
            r#"{"domain_suffix":["a.com","b.com"],"outbound":"proxy"}"#,
        );
        assert_eq!(info.field.as_deref(), Some("domain_suffix"));
        assert_eq!(info.value.as_deref(), Some("a.com, b.com"));
        assert_eq!(info.target.as_deref(), Some("proxy"));
    }

    #[test]
    fn describe_rule_falls_back_to_raw_for_unknown_shapes() {
        let raw = r#"{"rule_set":["myset"],"action":"route","outbound":"direct"}"#;
        let info = describe_rule(3, raw);
        assert!(info.field.is_none());
        assert_eq!(info.raw, raw);

        let broken = describe_rule(4, "not json");
        assert!(broken.field.is_none());
        assert_eq!(broken.raw, "not json");
    }

    #[tokio::test]
    async fn get_rules_marks_rules_skipped_at_generation() {
        use crate::models::Config;
        use crate::state::SkippedRule;
        use crate::test_support::app_state;
        use axum::extract::State;

        let raw_ok = r#"{"domain":"t.co","action":"route","outbound":"proxy"}"#.to_string();
        let raw_gone =
            r#"{"process_name":"nginx","action":"route","outbound":"ghost-node"}"#.to_string();
        let state = app_state(Config {
            custom_rules: vec![raw_ok, raw_gone.clone()],
            ..Default::default()
        });
        *state.skipped_rules.lock().await = vec![SkippedRule {
            raw: raw_gone,
            description: "process_name=nginx → ghost-node".to_string(),
        }];

        let axum::Json(response) = super::get_rules(State(state)).await;
        let rules = response.data.unwrap();
        assert!(!rules[0].skipped);
        assert!(rules[1].skipped);
    }

    #[test]
    fn describe_rule_falls_back_to_raw_for_compound_rules() {
        // 带有其他匹配条件的规则不做结构化展示,避免隐藏条件误导用户
        let raw = r#"{"domain_suffix":"example.com","network":"tcp","action":"route","outbound":"proxy"}"#;
        let info = describe_rule(5, raw);
        assert!(info.field.is_none());
        assert_eq!(info.raw, raw);

        // 两个匹配字段都在白名单内同样回退 raw
        let two_known = r#"{"domain_suffix":"google.com","process_name":"curl","action":"route","outbound":"direct"}"#;
        let info = describe_rule(6, two_known);
        assert!(info.field.is_none());
        assert_eq!(info.raw, two_known);
    }
}
