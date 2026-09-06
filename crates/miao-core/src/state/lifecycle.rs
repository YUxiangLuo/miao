use std::sync::Mutex;

use crate::models::RuntimePhase;

/// One coherent observation of intent, kernel ownership and data-plane health.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeSnapshot {
    pub generation: u64,
    pub phase: RuntimePhase,
    pub ready: bool,
    pub should_run: bool,
}

impl Default for RuntimeSnapshot {
    fn default() -> Self {
        Self {
            generation: 0,
            phase: RuntimePhase::Initializing,
            ready: false,
            should_run: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum KernelOperation {
    Start,
    // Windows uses a stop/start sequence instead of Unix SIGHUP.
    #[cfg_attr(not(unix), allow(dead_code))]
    Reload,
    Stop,
}

#[derive(Clone, Copy, Debug)]
pub enum RuntimeActivity {
    Extracting,
    Validating,
    ApplyingConfig,
}

#[derive(Default)]
pub struct RuntimeLifecycle {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    snapshot: RuntimeSnapshot,
    next_activity: u64,
    activity: Option<u64>,
    shutting_down: bool,
}

impl RuntimeLifecycle {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.inner.lock().unwrap().snapshot
    }

    /// Intent changes invalidate work issued under the previous intent, but
    /// do not pretend a still-live process has already stopped.
    pub fn request_running(&self, should_run: bool) {
        let mut inner = self.inner.lock().unwrap();
        if inner.shutting_down && should_run {
            return;
        }
        if inner.snapshot.should_run != should_run {
            inner.snapshot.should_run = should_run;
            inner.snapshot.generation += 1;
            inner.activity = None;
        }
    }

    /// Terminal for this AppState. Draining HTTP requests and rollback work
    /// may finish, but cannot request another kernel after server shutdown.
    pub fn shutdown(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.shutting_down = true;
        inner.snapshot.should_run = false;
        inner.snapshot.generation += 1;
        inner.activity = None;
    }

    /// The controller calls this while holding the process slot lock, before
    /// signalling/spawning/waiting. In particular Stop retires probes first.
    pub fn begin(&self, operation: KernelOperation) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.generation += 1;
        inner.snapshot.phase = match operation {
            KernelOperation::Start => RuntimePhase::Starting,
            KernelOperation::Reload => RuntimePhase::Reloading,
            KernelOperation::Stop => RuntimePhase::Stopping,
        };
        inner.snapshot.ready = false;
        inner.activity = None;
        inner.snapshot.generation
    }

    pub fn is_current(&self, generation: u64) -> bool {
        let snapshot = self.snapshot();
        snapshot.generation == generation && snapshot.should_run
    }

    /// Publish a small synchronous side effect (e.g. a warning) under the
    /// same ownership check. The callback must not reenter the lifecycle.
    pub fn with_current(&self, generation: u64, publish: impl FnOnce()) -> bool {
        let inner = self.inner.lock().unwrap();
        if inner.snapshot.generation != generation || !inner.snapshot.should_run {
            return false;
        }
        publish();
        true
    }

    /// Only the kernel controller may establish readiness after checking the
    /// owned child. Stale completions cannot modify any part of the snapshot.
    pub fn finish(&self, generation: u64, phase: RuntimePhase) -> bool {
        assert!(matches!(
            phase,
            RuntimePhase::Ready
                | RuntimePhase::Failed
                | RuntimePhase::Stopped
                | RuntimePhase::Starting
        ));
        let mut inner = self.inner.lock().unwrap();
        if inner.snapshot.generation != generation
            || (matches!(phase, RuntimePhase::Ready | RuntimePhase::Starting)
                && !inner.snapshot.should_run)
        {
            return false;
        }
        inner.snapshot.phase = phase;
        inner.snapshot.ready = phase == RuntimePhase::Ready;
        inner.activity = None;
        true
    }

    /// Configuration preparation never grants readiness. Its RAII projection
    /// restores the previous phase only if no kernel event superseded it.
    pub fn activity(&self, activity: RuntimeActivity) -> ActivityGuard<'_> {
        let mut inner = self.inner.lock().unwrap();
        let previous_phase = inner.snapshot.phase;
        let previous_activity = inner.activity;
        inner.next_activity += 1;
        let id = inner.next_activity;
        inner.activity = Some(id);
        inner.snapshot.phase = match activity {
            RuntimeActivity::Extracting => RuntimePhase::Extracting,
            RuntimeActivity::Validating => RuntimePhase::Validating,
            RuntimeActivity::ApplyingConfig => RuntimePhase::ApplyingConfig,
        };
        ActivityGuard {
            owner: self,
            id,
            previous_phase,
            previous_activity,
        }
    }
}

pub struct ActivityGuard<'a> {
    owner: &'a RuntimeLifecycle,
    id: u64,
    previous_phase: RuntimePhase,
    previous_activity: Option<u64>,
}

impl Drop for ActivityGuard<'_> {
    fn drop(&mut self) {
        let mut inner = self.owner.inner.lock().unwrap();
        if inner.activity == Some(self.id) {
            inner.snapshot.phase = self.previous_phase;
            inner.activity = self.previous_activity;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_is_terminal_but_an_ordinary_stop_is_restartable() {
        let state = RuntimeLifecycle::default();
        state.request_running(false);
        state.request_running(true);
        assert!(state.snapshot().should_run);
        let start = state.begin(KernelOperation::Start);
        state.shutdown();
        state.request_running(true);
        assert!(!state.snapshot().should_run);
        assert!(!state.finish(start, RuntimePhase::Ready));
    }

    #[test]
    fn stopped_runtime_rejects_late_success_and_failure() {
        let state = RuntimeLifecycle::default();
        let start = state.begin(KernelOperation::Start);
        let stop = state.begin(KernelOperation::Stop);
        assert!(state.finish(stop, RuntimePhase::Stopped));
        let stopped = state.snapshot();
        assert!(!state.finish(start, RuntimePhase::Ready));
        assert!(!state.finish(start, RuntimePhase::Failed));
        assert_eq!(state.snapshot(), stopped);
    }

    #[test]
    fn concurrent_readers_never_observe_torn_phase_and_readiness() {
        let state = std::sync::Arc::new(RuntimeLifecycle::default());
        let writer_state = state.clone();
        let writer = std::thread::spawn(move || {
            for _ in 0..2000 {
                let start = writer_state.begin(KernelOperation::Start);
                writer_state.finish(start, RuntimePhase::Ready);
                let stop = writer_state.begin(KernelOperation::Stop);
                writer_state.finish(stop, RuntimePhase::Stopped);
            }
        });
        for _ in 0..10000 {
            let snapshot = state.snapshot();
            assert_eq!(snapshot.ready, snapshot.phase == RuntimePhase::Ready);
        }
        writer.join().unwrap();
    }

    #[test]
    fn old_reload_cannot_change_a_new_instance() {
        let state = RuntimeLifecycle::default();
        let old = state.begin(KernelOperation::Reload);
        let new = state.begin(KernelOperation::Start);
        state.finish(new, RuntimePhase::Ready);
        assert!(!state.finish(old, RuntimePhase::Failed));
        assert!(state.snapshot().ready);
    }

    #[test]
    fn preparation_preserves_health_but_cannot_restore_it_after_a_crash() {
        let state = RuntimeLifecycle::default();
        let generation = state.begin(KernelOperation::Start);
        state.finish(generation, RuntimePhase::Ready);
        {
            let _outer = state.activity(RuntimeActivity::ApplyingConfig);
            {
                let _inner = state.activity(RuntimeActivity::Validating);
            }
            assert_eq!(state.snapshot().phase, RuntimePhase::ApplyingConfig);
            assert!(state.snapshot().ready);
        }
        assert_eq!(state.snapshot().phase, RuntimePhase::Ready);
        let preparation = state.activity(RuntimeActivity::ApplyingConfig);
        state.finish(generation, RuntimePhase::Failed);
        drop(preparation);
        assert_eq!(state.snapshot().phase, RuntimePhase::Failed);
        assert!(!state.snapshot().ready);
    }
}
