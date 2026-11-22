use std::sync::Arc;

use super::{
    command_error, success, success_no_data, CommandErrorKind, CommandReply, CommandResult,
};
use crate::models::{DeleteRuleRequest, RuleInfo, RuleRequest};
use crate::services::config::{known_rule_targets, ConfigEdit};
use crate::state::AppState;
use crate::validation::{Validator, CUSTOM_RULE_FIELDS};
use serde_json::{json, Map, Value as JsonValue};

/// 由 UI 表单生成规范的 sing-box 规则 JSON(action 格式)
pub(crate) fn build_rule_json(req: &RuleRequest) -> JsonValue {
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

struct StructuredRule {
    field: String,
    value: String,
    target: Option<String>,
}

fn parse_structured_rule(raw: &str) -> Option<StructuredRule> {
    let JsonValue::Object(map) = serde_json::from_str::<JsonValue>(raw).ok()? else {
        return None;
    };

    // 仅单条件规则做结构化展示:零个或多个已知匹配字段(含两个白名单字段的复合规则)
    // 都回退 raw,避免隐藏条件误导用户
    let mut matched = CUSTOM_RULE_FIELDS
        .iter()
        .copied()
        .filter(|field| map.contains_key(*field));
    let field = matched.next()?;
    if matched.next().is_some() {
        return None;
    }

    // 出现匹配字段/action/outbound 之外的键,同样回退 raw
    let has_unknown_keys = map
        .keys()
        .any(|key| key != field && key != "action" && key != "outbound");
    if has_unknown_keys {
        return None;
    }

    let value = match &map[field] {
        JsonValue::String(s) => s.clone(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Array(items) => items
            .iter()
            .filter_map(JsonValue::as_str)
            .collect::<Vec<_>>()
            .join(", "),
        _ => return None,
    };

    // 新格式 action 优先,兼容旧版 outbound 内联写法
    let target = match map.get("action").and_then(JsonValue::as_str) {
        Some("reject") => Some("reject".to_string()),
        _ => map
            .get("outbound")
            .and_then(JsonValue::as_str)
            .map(str::to_string),
    };

    Some(StructuredRule {
        field: field.to_string(),
        value,
        target,
    })
}

/// 把存储的 JSON 字符串还原成结构化展示;无法识别的手写规则以 raw 兜底
pub(crate) fn describe_rule(index: usize, raw: &str) -> RuleInfo {
    match parse_structured_rule(raw) {
        Some(structured) => RuleInfo {
            index,
            field: Some(structured.field),
            value: Some(structured.value),
            target: structured.target,
            skipped: false,
            raw: raw.to_string(),
        },
        None => RuleInfo {
            index,
            field: None,
            value: None,
            target: None,
            skipped: false,
            raw: raw.to_string(),
        },
    }
}

pub async fn get_rules(state: Arc<AppState>) -> CommandReply<Vec<RuleInfo>> {
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

pub async fn add_rule(state: Arc<AppState>, req: RuleRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    let mut edit = ConfigEdit::begin(&state).await;
    let extra_targets = known_rule_targets(edit.original(), &state).await;
    Validator::custom_rule(&req, &extra_targets)
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;

    let rule_json = build_rule_json(&req);
    let rule_str = serde_json::to_string(&rule_json)
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;

    if edit.candidate.custom_rules.contains(&rule_str) {
        return Err(command_error(CommandErrorKind::InvalidInput, "规则已存在"));
    }

    edit.candidate.custom_rules.push(rule_str);

    match edit.commit().await {
        Ok(_) => Ok(success_no_data("Rule added")),
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

pub async fn delete_rule(state: Arc<AppState>, req: DeleteRuleRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    let mut edit = ConfigEdit::begin(&state).await;

    let Some(existing) = edit.candidate.custom_rules.get(req.index) else {
        return Err(command_error(CommandErrorKind::NotFound, "Rule not found"));
    };
    // 列表可能因并发变更(其他标签页/轮询)前移,删除前必须确认条目未被挪动
    if existing != &req.raw {
        return Err(command_error(
            CommandErrorKind::Conflict,
            "规则列表已变化,请刷新后重试",
        ));
    }
    edit.candidate.custom_rules.remove(req.index);

    match edit.commit().await {
        Ok(_) => Ok(success_no_data("Rule deleted")),
        Err(e) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}
