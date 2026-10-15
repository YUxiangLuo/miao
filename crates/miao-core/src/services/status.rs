use crate::models::{
    NodeMultiplier, NodeSelect, RouteMode, RuntimeWarning, RuntimeWarningSeverity,
};
use crate::state::AppState;

/// Shared configuration projection for REST and MCP status responses.
/// `node_select` is effective; `requested_node_select` survives a
/// temporary regional fallback. The current cap remains an available option
/// even when no currently available node advertises that exact value. Node,
/// subscription and rule strings are not needed by status polling.
#[derive(Debug, PartialEq, Eq)]
pub struct RuntimeConfigStatus {
    pub route_mode: RouteMode,
    pub node_select: NodeSelect,
    pub max_multiplier: Option<NodeMultiplier>,
    pub mcp: bool,
    pub manual_node_count: usize,
    pub requested_node_select: NodeSelect,
    pub multiplier_options: Vec<NodeMultiplier>,
}

pub async fn runtime_config_status(state: &AppState) -> RuntimeConfigStatus {
    let (route_mode, node_select, max_multiplier, mcp, manual_node_count) = {
        let config = state.config.read().await;
        (
            config.route_mode,
            config.node_select,
            config.max_multiplier,
            config.mcp,
            config.nodes.len(),
        )
    };
    let requested_node_select = *state.node_select_preference.read().await;
    let mut multiplier_options = state.available_multipliers.read().await.clone();
    if let Some(current) = max_multiplier {
        if !multiplier_options.contains(&current) {
            multiplier_options.push(current);
            multiplier_options.sort_unstable();
        }
    }
    RuntimeConfigStatus {
        route_mode,
        node_select,
        max_multiplier,
        mcp,
        manual_node_count,
        requested_node_select,
        multiplier_options,
    }
}

#[cfg(test)]
mod benchmarks;

/// Build one diagnostic snapshot for REST and MCP. Keeping the compatibility
/// string as a projection of this list prevents the two control planes from
/// drifting as new warning kinds are added.
pub async fn runtime_warnings(state: &AppState) -> Vec<RuntimeWarning> {
    let mut warnings = Vec::new();
    if let Some(message) = state.config_warning.lock().await.clone() {
        warnings.push(RuntimeWarning {
            code: "runtime_config",
            message,
            severity: RuntimeWarningSeverity::Warning,
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
            severity: RuntimeWarningSeverity::Warning,
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
