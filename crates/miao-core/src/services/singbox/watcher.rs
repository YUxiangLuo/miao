use super::*;

/// 崩溃看门狗的巡检间隔。测试里缩短以保持用例快速。
#[cfg(not(test))]
const KERNEL_WATCH_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(test)]
const KERNEL_WATCH_INTERVAL: Duration = Duration::from_millis(200);

/// 连续自动重启超过此次数后放弃，并在面板上告警。
const MAX_KERNEL_RESTARTS: u32 = 5;

/// 内核存活超过该时长则认为已稳定，重置重启计数。
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
    state.sing_generation.load(Ordering::Relaxed) == expected_generation
        && state.service_should_run.load(Ordering::Relaxed)
}

pub(super) async fn clear_kernel_give_up_warning(state: &Arc<AppState>) {
    let mut warning = state.config_warning.lock().await;
    if warning.as_deref() == Some(KERNEL_GIVE_UP_WARNING) {
        *warning = None;
    }
}

/// 监护一次 sing-box 启动：异常退出时按退避自动拉起（复用当前 config.json，
/// 不重新生成）。有意停核/重启会递增 generation，看门狗随即退出。
pub(super) async fn watch_sing_box(state: Arc<AppState>, generation: u64) {
    let mut restarts = 0u32;
    loop {
        sleep(KERNEL_WATCH_INTERVAL).await;
        if state.sing_generation.load(Ordering::Relaxed) != generation {
            return;
        }

        let crashed = {
            let mut lock = state.sing_process.lock().await;
            match lock.as_mut() {
                Some(proc) => match proc.child.try_wait() {
                    Ok(None) => {
                        if proc.started_at.elapsed() >= KERNEL_STABLE_AFTER {
                            restarts = 0;
                        }
                        false
                    }
                    Ok(Some(status)) => {
                        warn!(exit_code = ?status.code(), "sing-box exited unexpectedly");
                        *lock = None;
                        state.runtime_ready.store(false, Ordering::Relaxed);
                        true
                    }
                    Err(err) => {
                        warn!(error = %err, "Failed to poll sing-box process state");
                        *lock = None;
                        state.runtime_ready.store(false, Ordering::Relaxed);
                        true
                    }
                },
                // 状态轮询等路径可能先收割了已退出的进程：槽位为空且
                // generation 未变，仍视为一次异常退出。
                None => true,
            }
        };
        if !crashed {
            continue;
        }
        if state.sing_generation.load(Ordering::Relaxed) != generation
            || !state.service_should_run.load(Ordering::Relaxed)
        {
            return;
        }

        restarts += 1;
        if restarts > MAX_KERNEL_RESTARTS {
            error!("sing-box kept crashing; giving up on automatic restarts");
            state.runtime_ready.store(false, Ordering::Relaxed);
            state.set_runtime_phase(RuntimePhase::Failed);
            *state.config_warning.lock().await = Some(KERNEL_GIVE_UP_WARNING.to_string());
            return;
        }

        state.set_runtime_phase(RuntimePhase::Starting);
        sleep(restart_backoff(restarts)).await;
        if state.sing_generation.load(Ordering::Relaxed) != generation
            || !state.service_should_run.load(Ordering::Relaxed)
        {
            return;
        }

        // Serialize crash recovery with user-driven config transactions. If a
        // settings update wins the lock during backoff, its new generation
        // retires this watcher before it can spawn the old bytes concurrently.
        let _config_update = state.config_update.lock().await;
        if state.sing_generation.load(Ordering::Relaxed) != generation
            || !state.service_should_run.load(Ordering::Relaxed)
        {
            return;
        }

        match spawn_and_probe_sing_box(&state, generation).await {
            Ok(()) => {
                state.runtime_ready.store(true, Ordering::Relaxed);
                state.set_runtime_phase(RuntimePhase::Ready);
                info!(restarts, "sing-box restarted after an unexpected exit");
            }
            Err(err) => {
                state.runtime_ready.store(false, Ordering::Relaxed);
                warn!(error = %err, "Failed to restart sing-box after an unexpected exit")
            }
        }
    }
}
