use std::sync::Mutex;
use tokio::time::{Duration, Instant};

use crate::models::{SubscriptionFetchReport, SubscriptionRefreshPhase, SubscriptionRefreshStatus};

/// Subscription activity has its own owner and never writes the proxy phase.
/// No lock is held across network I/O. Generation checks also apply to Drop,
/// so cancelling an old future cannot clear a newer request's status.
#[derive(Default)]
pub struct SubscriptionRefresh {
    inner: Mutex<RefreshState>,
    pub changed: tokio::sync::Notify,
}

#[derive(Default)]
struct RefreshState {
    generation: u64,
    attempt: u64,
    foreground_active: bool,
    status: SubscriptionRefreshStatus,
    retry_at: Option<Instant>,
}

impl SubscriptionRefresh {
    pub fn reset(&self, generation: u64) {
        let mut state = self.inner.lock().unwrap();
        if generation < state.generation {
            return;
        }
        state.generation = generation;
        state.attempt += 1;
        state.foreground_active = false;
        state.status = SubscriptionRefreshStatus::default();
        state.retry_at = None;
        self.changed.notify_waiters();
    }

    /// Held from before releasing config_update until the foreground commit
    /// (or rollback) finishes. A completed HTTP request is not yet a commit.
    pub fn foreground(&self, generation: u64) -> ForegroundGuard<'_> {
        let mut state = self.inner.lock().unwrap();
        if state.generation == generation {
            state.foreground_active = true;
        }
        ForegroundGuard {
            owner: self,
            generation,
        }
    }

    pub fn foreground_in_flight(&self) -> bool {
        self.inner.lock().unwrap().foreground_active
    }

    pub fn begin(&self, generation: u64) -> FetchGuard<'_> {
        let mut state = self.inner.lock().unwrap();
        if state.generation == generation {
            state.attempt += 1;
            state.status.phase = SubscriptionRefreshPhase::Fetching;
            state.retry_at = None;
        }
        FetchGuard {
            owner: self,
            generation,
            attempt: state.attempt,
        }
    }

    pub fn wait_to_retry(&self, generation: u64, delay: Duration) {
        let mut state = self.inner.lock().unwrap();
        if state.generation == generation
            && !state.foreground_active
            && state.status.phase != SubscriptionRefreshPhase::Fetching
        {
            state.status.phase = SubscriptionRefreshPhase::Waiting;
            state.retry_at = Some(Instant::now() + delay);
        }
    }

    pub fn snapshot(&self) -> SubscriptionRefreshStatus {
        let state = self.inner.lock().unwrap();
        SubscriptionRefreshStatus {
            retry_in_secs: state
                .retry_at
                .map(|at| at.saturating_duration_since(Instant::now()).as_secs()),
            ..state.status
        }
    }
}

pub struct ForegroundGuard<'a> {
    owner: &'a SubscriptionRefresh,
    generation: u64,
}

impl Drop for ForegroundGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.owner.inner.lock().unwrap();
        if state.generation == self.generation {
            state.foreground_active = false;
        }
        self.owner.changed.notify_waiters();
    }
}

pub struct FetchGuard<'a> {
    owner: &'a SubscriptionRefresh,
    generation: u64,
    attempt: u64,
}

impl FetchGuard<'_> {
    pub fn finish(self, report: SubscriptionFetchReport) {
        let mut state = self.owner.inner.lock().unwrap();
        if state.generation == self.generation && state.attempt == self.attempt {
            state.status = SubscriptionRefreshStatus {
                phase: if report.outcome() == crate::models::SubscriptionFetchOutcome::NotRequested
                {
                    SubscriptionRefreshPhase::Idle
                } else if report.total_failure() {
                    SubscriptionRefreshPhase::Failed
                } else {
                    SubscriptionRefreshPhase::Completed
                },
                outcome: report.outcome(),
                report,
                retry_in_secs: None,
            };
        }
    }
}

impl Drop for FetchGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.owner.inner.lock().unwrap();
        if state.generation == self.generation
            && state.attempt == self.attempt
            && state.status.phase == SubscriptionRefreshPhase::Fetching
        {
            state.status.phase = SubscriptionRefreshPhase::Idle;
        }
        self.owner.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_completion_and_cancellation_cannot_touch_a_new_fetch() {
        let tracker = SubscriptionRefresh::default();
        let old = tracker.begin(0);
        tracker.reset(1);
        let new = tracker.begin(1);
        old.finish(SubscriptionFetchReport {
            failed_sources: 1,
            ..Default::default()
        });
        assert_eq!(tracker.snapshot().phase, SubscriptionRefreshPhase::Fetching);
        drop(new);
        assert_eq!(tracker.snapshot().phase, SubscriptionRefreshPhase::Idle);
    }

    #[test]
    fn newer_attempt_in_the_same_generation_owns_the_status() {
        let tracker = SubscriptionRefresh::default();
        let old = tracker.begin(0);
        let new = tracker.begin(0);
        drop(old);
        assert_eq!(tracker.snapshot().phase, SubscriptionRefreshPhase::Fetching);
        new.finish(SubscriptionFetchReport {
            successful_sources: 1,
            ..Default::default()
        });
        assert_eq!(
            tracker.snapshot().phase,
            SubscriptionRefreshPhase::Completed
        );
    }

    #[test]
    fn retry_deadline_is_independent_and_reset_cancels_it() {
        let tracker = SubscriptionRefresh::default();
        tracker.wait_to_retry(0, Duration::from_secs(30));
        assert!(tracker
            .snapshot()
            .retry_in_secs
            .is_some_and(|remaining| remaining <= 30));
        tracker.reset(1);
        tracker.wait_to_retry(0, Duration::from_secs(30));
        assert_eq!(tracker.snapshot(), SubscriptionRefreshStatus::default());
    }
}
