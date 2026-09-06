use super::*;

#[cfg(not(test))]
const KERNEL_WATCH_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(test)]
const KERNEL_WATCH_INTERVAL: Duration = Duration::from_millis(200);
const MAX_KERNEL_RESTARTS: u32 = 5;
const KERNEL_STABLE_AFTER: Duration = Duration::from_secs(60);

pub(super) const KERNEL_GIVE_UP_WARNING: &str =
    "sing-box 反复异常退出，已停止自动拉起。请检查配置或查看日志";

pub(super) fn restart_backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << (attempt.saturating_sub(1)).min(4))
}

pub(super) fn spawn_crash_watcher(state: Arc<AppState>, generation: u64) {
    tokio::spawn(watch_sing_box(state, generation));
}

pub(super) fn start_still_current(state: &AppState, expected_generation: u64) -> bool {
    state.lifecycle.is_current(expected_generation)
}

pub(super) async fn clear_kernel_give_up_warning(state: &Arc<AppState>, generation: u64) {
    let mut warning = state.config_warning.lock().await;
    state.lifecycle.with_current(generation, || {
        if warning.as_deref() == Some(KERNEL_GIVE_UP_WARNING) {
            *warning = None;
        }
    });
}

/// Recheck ownership after obtaining the slot, not just before awaiting it.
/// Otherwise an old watcher can reap a new instance installed while it waited.
async fn poll_crash(state: &AppState, generation: u64, restarts: &mut u32) -> Option<bool> {
    let mut slot = state.sing_process.lock().await;
    if !start_still_current(state, generation) {
        return None;
    }
    // An empty slot may have been reaped by status polling already.
    if let Some(process) = slot.as_mut() {
        match process.child.try_wait() {
            Ok(None) => {
                if process.started_at.elapsed() >= KERNEL_STABLE_AFTER {
                    *restarts = 0;
                }
                return Some(false);
            }
            Ok(Some(status)) => warn!(exit_code = ?status.code(), "sing-box exited unexpectedly"),
            Err(err) => warn!(error = %err, "Failed to poll sing-box process state"),
        }
    }
    *slot = None;
    state.lifecycle.finish(generation, RuntimePhase::Failed);
    Some(true)
}

/// Recovery reuses config.json, preserves the retry budget and never owns a
/// newer start/reload/stop. Lock order: config_update -> process slot -> lifecycle.
pub(super) async fn watch_sing_box(state: Arc<AppState>, generation: u64) {
    let mut restarts = 0u32;
    loop {
        sleep(KERNEL_WATCH_INTERVAL).await;
        match poll_crash(&state, generation, &mut restarts).await {
            None => return,
            Some(false) => continue,
            Some(true) => {}
        }
        restarts += 1;
        if restarts <= MAX_KERNEL_RESTARTS {
            if !state.lifecycle.finish(generation, RuntimePhase::Starting) {
                return;
            }
            sleep(restart_backoff(restarts)).await;
        }

        let _config_update = state.config_update.lock().await;
        if !start_still_current(&state, generation) {
            return;
        }
        if restarts > MAX_KERNEL_RESTARTS {
            let mut warning = state.config_warning.lock().await;
            if state.lifecycle.with_current(generation, || {
                *warning = Some(KERNEL_GIVE_UP_WARNING.to_string());
            }) {
                error!("sing-box kept crashing; giving up on automatic restarts");
            }
            state.lifecycle.finish(generation, RuntimePhase::Failed);
            return;
        }
        let result = match spawn_and_probe_sing_box(&state, generation).await {
            Ok(()) => publish_kernel_ready(&state, generation).await,
            Err(err) => Err(err),
        };
        match result {
            Ok(()) => info!(restarts, "sing-box restarted after an unexpected exit"),
            Err(err) => {
                if !state.lifecycle.finish(generation, RuntimePhase::Failed) {
                    return;
                }
                warn!(error = %err, "Failed to restart sing-box after an unexpected exit");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_watcher_rechecks_after_waiting_for_the_process_slot() {
        let state = crate::test_support::app_state(crate::models::Config::default());
        let old = state.lifecycle.begin(KernelOperation::Start);
        let slot = state.sing_process.lock().await;
        let other = state.clone();
        let poll = tokio::spawn(async move { poll_crash(&other, old, &mut 0).await });
        tokio::task::yield_now().await;
        let new = state.lifecycle.begin(KernelOperation::Start);
        state.lifecycle.finish(new, RuntimePhase::Ready);
        let snapshot = state.lifecycle.snapshot();
        drop(slot);
        assert_eq!(poll.await.unwrap(), None);
        assert_eq!(state.lifecycle.snapshot(), snapshot);
    }
}
