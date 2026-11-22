use std::sync::Mutex;

/// Recovery ownership is independent of mutation admission. A background owner
/// may sleep indefinitely while offline; background recovery only registers
/// bounded attempts, so shutdown/upgrade never waits for the entire retry loop.
#[derive(Default)]
pub(crate) struct SubscriptionRecovery {
    /// Coalesce startup and manual-start handoffs into one background loop.
    /// Queued entrants recheck completion/generation when they acquire ownership.
    pub background: tokio::sync::Mutex<()>,
    /// Serialize ALL recovery fetch/install attempts, including one-shot manual
    /// recovery. Acquired before the attempt's mutation lease and config_update;
    /// a foreground subscription edit remains independent and supersedes via
    /// sub_refresh_generation rather than waiting for this network I/O.
    pub attempt: tokio::sync::Mutex<()>,
    /// Unlike HTTP completion, this is recorded after a successful commit (or
    /// an accepted empty response that deliberately keeps the healthy runtime).
    /// Option makes generation zero a valid completed startup generation.
    completed_generation: Mutex<Option<u64>>,
}

impl SubscriptionRecovery {
    pub fn is_complete(&self, generation: u64) -> bool {
        *self.completed_generation.lock().unwrap() == Some(generation)
    }

    pub fn complete(&self, generation: u64) {
        *self.completed_generation.lock().unwrap() = Some(generation);
    }

    /// A new repair of an unavailable data plane must not inherit the previous
    /// completion, even if its subscription inputs/generation are unchanged.
    pub fn invalidate(&self) {
        *self.completed_generation.lock().unwrap() = None;
    }
}
