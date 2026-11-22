use crate::models::{ApiResponse, DeleteRuleRequest, RuleInfo, RuleRequest};
use crate::responses::{command_reply, command_result, HandlerResult};
use crate::services::commands;
use crate::state::AppState;
use axum::{extract::State, response::Json};
#[cfg(test)]
use commands::rules::{build_rule_json, describe_rule};
use std::sync::Arc;

pub async fn get_rules(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<RuleInfo>>> {
    command_reply(commands::rules::get_rules(state).await)
}

pub async fn add_rule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RuleRequest>,
) -> HandlerResult {
    command_result(commands::rules::add_rule(state, req).await)
}

pub async fn delete_rule(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteRuleRequest>,
) -> HandlerResult {
    command_result(commands::rules::delete_rule(state, req).await)
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
