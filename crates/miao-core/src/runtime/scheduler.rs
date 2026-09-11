//! 定时刷新订阅的调度循环。持久配置是稳定层 `scheduled_refresh`，
//! 时刻按运行主机的系统本地时区解释；服务关闭或配置变化通过 Notify 唤醒重算。
//!
//! 刷新失败（或到点时初始化未完成/已有前台刷新）时按 [`RETRY_BACKOFF`] 有界退避重试；
//! 成功、无需执行或新的计划时刻都会清零预算，用尽后退回到「等下一个计划时刻」。
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::services::config::{refresh_subscriptions_foreground, ConfigMutationError};
use crate::services::schedule;
use crate::state::AppState;

/// 单次睡眠上限：系统时钟或夏令时变化后，最迟这么久重算目标时刻。
const MAX_TICK: Duration = Duration::from_secs(30);

/// 失败后的退避重试间隔：第 1/2/3 次失败后分别等待 1/5/15 分钟，之后等下一个计划时刻。
/// 重试不会越过下一个计划时刻；成功、无需执行或新的计划时刻都会清零预算。
const RETRY_BACKOFF: &[Duration] = &[
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
];

pub(super) fn spawn(state: Arc<AppState>) {
    tokio::spawn(run_scheduled_refreshes(
        state,
        RetryState::new(RETRY_BACKOFF.to_vec()),
    ));
}

/// 一次到点动作的结果，决定是否进入退避重试。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FireOutcome {
    /// 刷新完成，且不是全部来源失败（含权威空列表）。
    Succeeded,
    /// 本次值得退避重试：初始化中、已有前台刷新、刷新失败或全部来源失败。
    Retryable,
    /// 本次不必执行、也不必重试：没有配置订阅，或已被更新的前台操作取代。
    Skipped,
}

/// 失败退避预算。`retry_at` 必须存绝对时刻：循环每 [`MAX_TICK`] 会重算目标，
/// 按剩余时长存储会被不断推后，永远到不了点。
struct RetryState {
    backoff: Vec<Duration>,
    failures: usize,
    retry_at: Option<DateTime<Local>>,
}

impl RetryState {
    fn new(backoff: Vec<Duration>) -> Self {
        Self {
            backoff,
            failures: 0,
            retry_at: None,
        }
    }

    /// 测试用：预置一次「现在到期」的重试，无需等待真实计划时刻。
    #[cfg(test)]
    fn due_now(backoff: Vec<Duration>) -> Self {
        Self {
            retry_at: Some(Local::now()),
            ..Self::new(backoff)
        }
    }

    /// 本次等待的目标：未完成的退避优先，但计划时刻更早时以计划时刻为准。
    /// 返回 `(目标时刻, 是否退避重试)`。
    fn target(&self, scheduled: DateTime<Local>) -> (DateTime<Local>, bool) {
        match self.retry_at {
            Some(retry) if retry < scheduled => (retry, true),
            _ => (scheduled, false),
        }
    }

    /// 到点执行前的准备：计划时刻（非退避）开始新一轮预算。
    fn begin(&mut self, is_retry: bool) {
        if !is_retry {
            self.clear();
        }
    }

    fn on_success(&mut self) {
        self.clear();
    }

    /// 记录一次可重试的失败并推进退避；返回是否还有重试预算。
    fn on_failure(&mut self) -> bool {
        self.retry_at = self
            .backoff
            .get(self.failures)
            .copied()
            .map(|delay| Local::now() + chrono::Duration::from_std(delay).unwrap_or_default());
        self.failures = self.failures.saturating_add(1);
        self.retry_at.is_some()
    }

    fn clear(&mut self) {
        self.failures = 0;
        self.retry_at = None;
    }
}

async fn run_scheduled_refreshes(state: Arc<AppState>, mut retry: RetryState) {
    loop {
        let Some(scheduled) = next_target(&state).await else {
            // 未启用/没有有效时刻：清空退避，只等配置或关闭唤醒。
            retry.clear();
            if !wait_for_event(&state, None).await {
                info!("Scheduled refresh scheduler stopped");
                return;
            }
            continue;
        };

        let (target, is_retry) = retry.target(scheduled);
        let wait = (target - Local::now())
            .to_std()
            .unwrap_or_default()
            .min(MAX_TICK);
        if !wait_for_event(&state, Some(wait)).await {
            info!("Scheduled refresh scheduler stopped");
            return;
        }
        // 睡眠按块进行：到点或时钟跳变越过后触发；否则继续等待同一个绝对目标。
        if Local::now() < target {
            continue;
        }
        retry.begin(is_retry);

        match fire(&state).await {
            FireOutcome::Succeeded | FireOutcome::Skipped => retry.on_success(),
            FireOutcome::Retryable => {
                if retry.on_failure() {
                    let retry_in_secs = retry
                        .retry_at
                        .map(|at| (at - Local::now()).num_seconds().max(0))
                        .unwrap_or(0);
                    warn!(
                        retry_in_secs,
                        attempts = retry.failures,
                        "Scheduled subscription refresh failed; retrying with backoff"
                    );
                } else {
                    warn!(
                        "Scheduled subscription refresh retries exhausted; waiting for the next scheduled time"
                    );
                }
            }
        }
    }
}

/// 下一次计划执行时刻；未启用或没有有效时刻时为 None。
async fn next_target(state: &Arc<AppState>) -> Option<DateTime<Local>> {
    let schedule = state.stable_config.read().await.scheduled_refresh.clone();
    if !schedule.enabled {
        return None;
    }
    schedule::next_run_at(&schedule::parse_times(&schedule.times), Local::now())
}

/// 等待配置变化、订阅变更、关闭信号或超时；返回 false 表示服务进入关闭流程。
async fn wait_for_event(state: &Arc<AppState>, duration: Option<Duration>) -> bool {
    // 先注册再检查状态，避免关闭通知在两者之间丢失。
    let wake = state.scheduled_refresh_wake.notified();
    let cancel = state.sub_refresh_cancel.notified();
    tokio::pin!(wake);
    tokio::pin!(cancel);
    wake.as_mut().enable();
    cancel.as_mut().enable();
    if state.lifecycle.is_shutting_down() {
        return false;
    }
    match duration {
        Some(duration) => {
            tokio::select! {
                _ = &mut wake => {}
                _ = &mut cancel => {}
                _ = sleep(duration) => {}
            }
        }
        None => {
            tokio::select! {
                _ = &mut wake => {}
                _ = &mut cancel => {}
            }
        }
    }
    !state.lifecycle.is_shutting_down()
}

/// 触发一次定时刷新。复用面板手动刷新的业务路径（生成、校验、热应用、状态投影），
/// 失败不向调用方返回 HTTP 错误，只把结果映射成是否退避重试。
async fn fire(state: &Arc<AppState>) -> FireOutcome {
    if state.initializing.load(Ordering::Relaxed) {
        info!("Scheduled refresh deferred: initialization is still in progress");
        return FireOutcome::Retryable;
    }
    if state.subscription_refresh.foreground_in_flight() {
        info!("Scheduled refresh deferred: another refresh is already in flight");
        return FireOutcome::Retryable;
    }
    if state.config.read().await.subs.is_empty() {
        info!("Scheduled refresh skipped: no subscriptions configured");
        return FireOutcome::Skipped;
    }

    info!("Scheduled subscription refresh triggered");
    match refresh_subscriptions_foreground(state).await {
        Ok(update) => {
            // 口径与启动恢复一致：成功和失败来源都有（partial_failure）属于被接受的
            // 刷新——失败来源沿用缓存节点、成功来源已提交，不退避重试。
            // 只有全部来源失败（total_failure）才当作失败，网络恢复后不用等下个计划时刻。
            let refresh = state.subscription_refresh.snapshot();
            if refresh.report.total_failure() {
                warn!(
                    update = ?update,
                    "Scheduled subscription refresh failed for every source; keeping current runtime"
                );
                FireOutcome::Retryable
            } else {
                info!(update = ?update, "Scheduled subscription refresh finished");
                FireOutcome::Succeeded
            }
        }
        Err(ConfigMutationError::Superseded) => {
            info!("Scheduled refresh superseded by a newer foreground operation");
            FireOutcome::Skipped
        }
        Err(err) => {
            warn!(error = %err, "Scheduled subscription refresh failed");
            FireOutcome::Retryable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Config, ScheduledRefresh};
    use crate::services::schedule::TIME_FORMAT;
    use crate::test_support::isolated_stopped_state;

    fn local(hour: u32, minute: u32) -> DateTime<Local> {
        use chrono::TimeZone;
        Local
            .with_ymd_and_hms(2026, 9, 11, hour, minute, 0)
            .earliest()
            .expect("test local time is constructible")
    }

    #[test]
    fn retry_budget_walks_the_backoff_then_exhausts() {
        let mut retry = RetryState::new(vec![Duration::from_secs(1), Duration::from_secs(2)]);
        assert!(retry.retry_at.is_none());

        assert!(retry.on_failure());
        assert!(retry.retry_at.is_some());
        assert!(retry.on_failure());
        assert!(!retry.on_failure(), "预算用尽后不再安排重试");
        assert!(retry.retry_at.is_none());
        assert!(!retry.on_failure());

        retry.on_success();
        assert!(retry.on_failure(), "成功清零后重新获得完整预算");
        assert_eq!(retry.failures, 1);
    }

    #[test]
    fn retry_target_never_crosses_the_next_scheduled_time() {
        let now = local(12, 0);
        let mut retry = RetryState::new(vec![Duration::from_secs(60)]);
        retry.retry_at = Some(now + chrono::Duration::minutes(1));

        let later = now + chrono::Duration::hours(1);
        assert_eq!(
            retry.target(later),
            (now + chrono::Duration::minutes(1), true)
        );

        let earlier = now + chrono::Duration::seconds(30);
        assert_eq!(retry.target(earlier), (earlier, false));
        retry.begin(false);
        assert!(retry.retry_at.is_none(), "计划时刻开始新一轮预算");
    }

    #[tokio::test]
    async fn disabled_schedule_waits_for_events_and_stops_on_shutdown() {
        let (_root, state) = isolated_stopped_state(Config::default());
        let cloned = state.clone();
        let task = tokio::spawn(run_scheduled_refreshes(cloned, RetryState::new(Vec::new())));

        // 未启用：没有任何目标时刻，但循环应保持存活
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!task.is_finished());

        // 关闭信号必须让循环退出（不依赖配置变化）
        state.lifecycle.shutdown();
        state.scheduled_refresh_wake.notify_waiters();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("scheduler exits on shutdown")
            .expect("scheduler task joins");
    }

    #[tokio::test]
    async fn next_target_reads_the_current_schedule() {
        let (_root, state) = isolated_stopped_state(Config::default());
        assert!(next_target(&state).await.is_none());

        state.stable_config.write().await.scheduled_refresh = ScheduledRefresh {
            enabled: true,
            times: vec!["00:00".to_string()],
        };
        let target = next_target(&state)
            .await
            .expect("enabled schedule has target");
        // 目标是下一个 00:00，必然晚于当前本地时间
        assert!(target > Local::now());
        assert_eq!(
            target.time().format(TIME_FORMAT).to_string(),
            "00:00".to_string()
        );
    }

    #[tokio::test]
    async fn fire_skips_without_subscriptions_and_retries_unreachable_sources() {
        let (_root, state) = isolated_stopped_state(Config::default());
        assert_eq!(fire(&state).await, FireOutcome::Skipped);

        let config = Config {
            subs: vec!["http://127.0.0.1:9/unreachable".to_string()],
            ..Config::default()
        };
        let (_unreachable_root, unreachable) = isolated_stopped_state(config);
        assert_eq!(fire(&unreachable).await, FireOutcome::Retryable);
        assert!(unreachable
            .subscription_refresh
            .snapshot()
            .report
            .total_failure());
    }

    #[tokio::test]
    async fn scheduled_failures_retry_within_budget_then_stop() {
        let config = Config {
            subs: vec!["http://127.0.0.1:9/unreachable".to_string()],
            ..Config::default()
        };
        let (_root, state) = isolated_stopped_state(config);
        state.stable_config.write().await.scheduled_refresh = ScheduledRefresh {
            enabled: true,
            times: vec!["00:00".to_string()],
        };
        let cloned = state.clone();
        // 预置一次「现在到期」的重试，触发首次失败；预算两步 ⇒ 共 3 次尝试。
        // 计划时刻设为下一个 00:00，退避（20ms）显然更早，不会被计划时刻截断。
        let task = tokio::spawn(run_scheduled_refreshes(
            cloned,
            RetryState::due_now(vec![Duration::from_millis(20), Duration::from_millis(20)]),
        ));

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while state.sub_refresh_generation.load(Ordering::Relaxed) < 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "expected the first attempt plus two backoff retries"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // 预算用尽：不再产生新尝试，等下一个计划时刻
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert_eq!(state.sub_refresh_generation.load(Ordering::Relaxed), 3);

        state.lifecycle.shutdown();
        state.scheduled_refresh_wake.notify_waiters();
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("scheduler exits on shutdown")
            .expect("scheduler task joins");
    }

    /// 部分来源失败是与启动恢复一致的可接受结果：正常提交、不退避重试。
    #[cfg(unix)]
    #[tokio::test]
    async fn partial_source_failure_is_accepted_without_retry() {
        use crate::models::SubscriptionFetchOutcome;
        use axum::{routing::get, Router};
        use std::os::unix::fs::PermissionsExt;

        let app = Router::new().route(
            "/sub",
            get(|| async {
                "proxies:\n  - name: hk-01\n    type: hysteria2\n    server: 127.0.0.1\n    port: 443\n    password: test\n"
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let config = Config {
            subs: vec![
                format!("http://{addr}/sub"),
                "http://127.0.0.1:9/unreachable".to_string(),
            ],
            ..Config::default()
        };
        let (_root, state) = isolated_stopped_state(config);
        // 停服路径只跑 `check`，不会启动内核；假内核让候选校验通过。
        let kernel = state.runtime_paths.runtime_dir.join("sing-box");
        tokio::fs::write(&kernel, b"#!/bin/sh\n[ \"$1\" = check ]\n")
            .await
            .unwrap();
        std::fs::set_permissions(&kernel, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(fire(&state).await, FireOutcome::Succeeded);
        let refresh = state.subscription_refresh.snapshot();
        assert_eq!(refresh.outcome, SubscriptionFetchOutcome::PartialFailure);
        assert_eq!(refresh.report.successful_sources, 1);
        assert_eq!(refresh.report.failed_sources, 1);

        server.abort();
    }
}
