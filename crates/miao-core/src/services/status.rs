use crate::models::RuntimeWarning;
use crate::state::AppState;

/// Build one diagnostic snapshot for REST and MCP. Keeping the compatibility
/// string as a projection of this list prevents the two control planes from
/// drifting as new warning kinds are added.
pub async fn runtime_warnings(state: &AppState) -> Vec<RuntimeWarning> {
    let mut warnings = Vec::new();
    if let Some(message) = state.config_warning.lock().await.clone() {
        warnings.push(RuntimeWarning {
            code: "runtime_config",
            message,
            severity: "warning",
        });
    }

    let skipped_rules = state.skipped_rules.lock().await;
    if !skipped_rules.is_empty() {
        warnings.push(RuntimeWarning {
            code: "custom_rule_outbound_missing",
            message: format!(
                "{} 条自定义规则因出口节点不存在已跳过: {}",
                skipped_rules.len(),
                skipped_rules
                    .iter()
                    .map(|rule| rule.description.as_str())
                    .collect::<Vec<_>>()
                    .join(";")
            ),
            severity: "warning",
        });
    }
    warnings
}

pub fn legacy_warning(warnings: &[RuntimeWarning]) -> Option<String> {
    (!warnings.is_empty()).then(|| {
        warnings
            .iter()
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join(";")
    })
}

#[cfg(test)]
mod tests {
    use crate::{models::Config, state::SkippedRule, test_support::app_state};

    use super::{legacy_warning, runtime_warnings};

    #[tokio::test]
    async fn structured_warnings_keep_legacy_projection() {
        let state = app_state(Config::default());
        *state.config_warning.lock().await = Some("配置降级".to_string());
        state.skipped_rules.lock().await.push(SkippedRule {
            raw: "{}".to_string(),
            description: "domain=a.example → gone".to_string(),
        });

        let warnings = runtime_warnings(&state).await;
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0].code, "runtime_config");
        assert_eq!(warnings[1].code, "custom_rule_outbound_missing");
        assert_eq!(
            legacy_warning(&warnings).as_deref(),
            Some("配置降级;1 条自定义规则因出口节点不存在已跳过: domain=a.example → gone")
        );
    }
}
