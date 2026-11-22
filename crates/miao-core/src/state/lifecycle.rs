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

impl RuntimeSnapshot {
    /// `Ready` and `Starting` require active run intent; `Failed` and `Stopped`
    /// may still settle the current generation when intent is already false.
    fn can_finish_as(self, generation: u64, phase: RuntimePhase) -> bool {
        if self.generation != generation {
            return false;
        }
        match phase {
            RuntimePhase::Ready | RuntimePhase::Starting => self.should_run,
            RuntimePhase::Failed | RuntimePhase::Stopped => true,
            _ => unreachable!("unsupported terminal runtime phase: {phase:?}"),
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

impl KernelOperation {
    fn phase(self) -> RuntimePhase {
        match self {
            Self::Start => RuntimePhase::Starting,
            Self::Reload => RuntimePhase::Reloading,
            Self::Stop => RuntimePhase::Stopping,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum RuntimeActivity {
    Extracting,
    Validating,
    ApplyingConfig,
}

impl RuntimeActivity {
    fn phase(self) -> RuntimePhase {
        match self {
            Self::Extracting => RuntimePhase::Extracting,
            Self::Validating => RuntimePhase::Validating,
            Self::ApplyingConfig => RuntimePhase::ApplyingConfig,
        }
    }
}

/// Three-state write-admission and upgrade-handoff gate. `ShuttingDown` is
/// terminal: dropping an `UpgradeQuiescence` guard after shutdown never reopens
/// write admission or permits `exec` handoff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MutationAdmission {
    #[default]
    Open,
    #[cfg_attr(windows, allow(dead_code))]
    QuiescingForUpgrade,
    ShuttingDown,
}

impl MutationAdmission {
    fn allows_mutations(self) -> bool {
        matches!(self, Self::Open)
    }

    fn is_shutting_down(self) -> bool {
        matches!(self, Self::ShuttingDown)
    }

    #[cfg_attr(windows, allow(dead_code))]
    fn allows_upgrade_handoff(self) -> bool {
        matches!(self, Self::QuiescingForUpgrade)
    }

    #[cfg_attr(windows, allow(dead_code))]
    fn enter_quiescence(&mut self) -> bool {
        if matches!(self, Self::Open) {
            *self = Self::QuiescingForUpgrade;
            true
        } else {
            false
        }
    }

    #[cfg_attr(windows, allow(dead_code))]
    fn resume_from_quiescence(&mut self) {
        if matches!(self, Self::QuiescingForUpgrade) {
            *self = Self::Open;
        }
    }

    fn enter_shutdown(&mut self) {
        *self = Self::ShuttingDown;
    }
}

#[derive(Default)]
pub struct RuntimeLifecycle {
    inner: Mutex<Inner>,
    shutdown_started: tokio::sync::Notify,
    mutations_finished: tokio::sync::Notify,
}

#[derive(Default)]
struct Inner {
    snapshot: RuntimeSnapshot,
    next_activity: u64,
    activity: Option<u64>,
    admission: MutationAdmission,
    active_mutations: usize,
}

impl RuntimeLifecycle {
    pub fn snapshot(&self) -> RuntimeSnapshot {
        self.inner.lock().unwrap().snapshot
    }

    /// Intent changes invalidate work issued under the previous intent, but
    /// do not pretend a still-live process has already stopped.
    pub fn request_running(&self, should_run: bool) {
        let mut inner = self.inner.lock().unwrap();
        if inner.admission.is_shutting_down() && should_run {
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
        inner.admission.enter_shutdown();
        inner.snapshot.should_run = false;
        inner.snapshot.generation += 1;
        inner.activity = None;
        self.shutdown_started.notify_waiters();
    }

    /// Request-body reception can be cancelled before any write is admitted.
    /// Register before checking the terminal flag to avoid missing shutdown.
    pub(crate) async fn wait_for_shutdown(&self) {
        let shutdown = self.shutdown_started.notified();
        tokio::pin!(shutdown);
        shutdown.as_mut().enable();
        if !self.is_shutting_down() {
            shutdown.await;
        }
    }

    /// Admission and shutdown share a lock: every accepted write is counted
    /// before shutdown can observe an empty set; later writes are rejected.
    pub(super) fn register_mutation(&self) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if !inner.admission.allows_mutations() {
            return false;
        }
        inner.active_mutations += 1;
        true
    }

    pub(super) fn finish_mutation(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.active_mutations -= 1;
        self.mutations_finished.notify_waiters();
    }

    /// Call after shutdown closes admission, without holding config_update:
    /// accepted requests may still need that lock to commit or roll back.
    pub(crate) async fn wait_for_mutations(&self) {
        self.wait_for_mutations_except(0).await;
    }

    pub(super) async fn wait_for_mutations_except(&self, exempt: usize) {
        loop {
            let finished = self.mutations_finished.notified();
            tokio::pin!(finished);
            finished.as_mut().enable();
            if self.inner.lock().unwrap().active_mutations == exempt {
                return;
            }
            finished.await;
        }
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(super) fn quiesce_mutations(&self) -> bool {
        self.inner.lock().unwrap().admission.enter_quiescence()
    }

    #[cfg_attr(windows, allow(dead_code))]
    pub(super) fn resume_mutations(&self) {
        self.inner
            .lock()
            .unwrap()
            .admission
            .resume_from_quiescence();
    }

    #[cfg(test)]
    pub(crate) fn mutations_allowed(&self) -> bool {
        self.inner.lock().unwrap().admission.allows_mutations()
    }

    /// No await in this critical section. Either shutdown wins and prevents
    /// exec, or successful exec replaces the process before shutdown begins.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) fn with_upgrade_handoff<T>(&self, action: impl FnOnce() -> T) -> Option<T> {
        let inner = self.inner.lock().unwrap();
        if !inner.admission.allows_upgrade_handoff() {
            return None;
        }
        Some(action())
    }

    /// The controller calls this while holding the process slot lock, before
    /// signalling/spawning/waiting. In particular Stop retires probes first.
    pub fn begin(&self, operation: KernelOperation) -> u64 {
        let mut inner = self.inner.lock().unwrap();
        inner.snapshot.generation += 1;
        inner.snapshot.phase = operation.phase();
        inner.snapshot.ready = false;
        inner.activity = None;
        inner.snapshot.generation
    }

    /// 服务是否已进入不可逆关闭流程；后台任务据此退出等待。
    pub fn is_shutting_down(&self) -> bool {
        self.inner.lock().unwrap().admission.is_shutting_down()
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
        if !inner.snapshot.can_finish_as(generation, phase) {
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
        inner.snapshot.phase = activity.phase();
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

    #[test]
    fn mutation_admission_transitions_preserve_terminal_shutdown() {
        let mut admission = MutationAdmission::default();
        assert!(admission.allows_mutations());
        assert!(!admission.allows_upgrade_handoff());

        assert!(admission.enter_quiescence());
        assert!(!admission.enter_quiescence());
        assert!(!admission.allows_mutations());
        assert!(admission.allows_upgrade_handoff());

        admission.resume_from_quiescence();
        assert!(admission.allows_mutations());
        assert!(!admission.allows_upgrade_handoff());

        assert!(admission.enter_quiescence());
        admission.enter_shutdown();
        assert!(admission.is_shutting_down());
        assert!(!admission.allows_upgrade_handoff());
        admission.resume_from_quiescence();
        assert!(admission.is_shutting_down());
        assert!(!admission.allows_mutations());
    }
}
