use serde::Serialize;

/// Network/content health, independent of cached nodes and selection filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionFetchOutcome {
    #[default]
    NotRequested,
    Success,
    Empty,
    PartialFailure,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubscriptionFetchReport {
    pub successful_sources: usize,
    pub failed_sources: usize,
    /// Before user disable/region/multiplier filters.
    pub fresh_nodes: usize,
    pub cached_nodes: usize,
}

impl SubscriptionFetchReport {
    pub fn outcome(self) -> SubscriptionFetchOutcome {
        match (
            self.successful_sources,
            self.failed_sources,
            self.fresh_nodes,
        ) {
            (0, 0, _) => SubscriptionFetchOutcome::NotRequested,
            (0, _, _) => SubscriptionFetchOutcome::Failed,
            (_, 0, 0) => SubscriptionFetchOutcome::Empty,
            (_, 0, _) => SubscriptionFetchOutcome::Success,
            _ => SubscriptionFetchOutcome::PartialFailure,
        }
    }

    pub fn total_failure(self) -> bool {
        self.outcome() == SubscriptionFetchOutcome::Failed
    }

    /// A successful empty list is authoritative too. Cached nodes alone are not.
    pub fn accepted_response(self) -> bool {
        self.successful_sources > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionFailureKind {
    Network,
    Http,
    Timeout,
    Parse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionRefreshPhase {
    #[default]
    Idle,
    Fetching,
    Waiting,
    Completed,
    Failed,
}

/// Fetch activity only; does not claim that the candidate was activated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubscriptionRefreshStatus {
    pub phase: SubscriptionRefreshPhase,
    pub outcome: SubscriptionFetchOutcome,
    pub report: SubscriptionFetchReport,
    pub retry_in_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_is_not_fetch_health() {
        let cases = [
            ((0, 0, 0, 0), SubscriptionFetchOutcome::NotRequested),
            ((0, 1, 0, 3), SubscriptionFetchOutcome::Failed),
            ((1, 0, 0, 0), SubscriptionFetchOutcome::Empty),
            ((1, 0, 3, 0), SubscriptionFetchOutcome::Success),
            ((1, 1, 0, 3), SubscriptionFetchOutcome::PartialFailure),
            ((1, 1, 3, 2), SubscriptionFetchOutcome::PartialFailure),
        ];
        for ((successful_sources, failed_sources, fresh_nodes, cached_nodes), expected) in cases {
            let report = SubscriptionFetchReport {
                successful_sources,
                failed_sources,
                fresh_nodes,
                cached_nodes,
            };
            assert_eq!(report.outcome(), expected);
            assert_eq!(report.accepted_response(), successful_sources > 0);
        }
    }
}
