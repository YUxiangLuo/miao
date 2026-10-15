use std::future::Future;

use super::*;
use tokio::time::Instant;

const PROBE_INTERVAL: Duration = Duration::from_millis(25);
const CLASH_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
// OpenWrt can need several seconds to install auto_redirect and bind Clash.
const READY_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Start,

    Reload { pid: u32 },
}

impl Operation {
    fn settle_time(self) -> Duration {
        match self {
            Self::Start => Duration::from_millis(100),

            Self::Reload { .. } => Duration::from_millis(500),
        }
    }

    fn cancelled(self) -> &'static str {
        match self {
            Self::Start => "sing-box start was cancelled",

            Self::Reload { .. } => "sing-box reload was cancelled",
        }
    }

    fn timeout(self) -> &'static str {
        match self {
            Self::Start => {
                "sing-box process started but its data plane did not become ready within 8 seconds"
            }

            Self::Reload { .. } => {
                "sing-box data plane did not become ready within 8 seconds after reload"
            }
        }
    }
}

// Only observations are replaced in hermetic tests. Timing, consecutive
// successes, ownership checks and timeout cleanup all use the production loop.
trait DataPlaneProbe {
    fn tun_ready(&self) -> bool;
    fn clash_ready(&mut self) -> impl Future<Output = bool> + Send;
}

struct SystemProbe<'a> {
    client: &'a reqwest::Client,
    clash_url: String,
}

impl DataPlaneProbe for SystemProbe<'_> {
    fn tun_ready(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new("/sys/class/net/sing-tun").exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }

    async fn clash_ready(&mut self) -> bool {
        self.client
            .get(&self.clash_url)
            .timeout(CLASH_PROBE_TIMEOUT)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
    }
}

pub(super) async fn wait_for_ready(
    state: &Arc<AppState>,
    generation: u64,
    operation: Operation,
) -> AppResult<()> {
    #[cfg(not(test))]
    let mut probe = SystemProbe {
        client: &state.http_client,
        clash_url: clash_api_url("/version"),
    };
    #[cfg(test)]
    let mut probe = tests::ReadyProbe;

    wait_with_probe(state, generation, operation, &mut probe).await
}

async fn check_process(state: &AppState, generation: u64, operation: Operation) -> AppResult<()> {
    let mut slot = state.sing_process.lock().await;
    if !start_still_current(state, generation) {
        return Err(AppError::message(operation.cancelled()));
    }
    let Some(process) = slot.as_mut() else {
        return Err(AppError::message(match operation {
            Operation::Start => operation.cancelled(),

            Operation::Reload { .. } => "sing-box exited during reload",
        }));
    };

    if let Operation::Reload { pid } = operation {
        if process.child.id() != Some(pid) {
            return Err(AppError::message(
                "sing-box process changed unexpectedly during reload",
            ));
        }
    }
    let context = match operation {
        Operation::Start => "Failed to check sing-box startup status",

        Operation::Reload { .. } => "Failed to check sing-box reload status",
    };
    if let Some(status) = process
        .child
        .try_wait()
        .map_err(|e| AppError::context(context, e))?
    {
        *slot = None;

        let phase = match operation {
            Operation::Start => "startup",

            Operation::Reload { .. } => "reload",
        };
        return Err(AppError::message(format!(
            "sing-box exited during {phase} with code {}",
            status.code().unwrap_or(-1)
        )));
    }
    Ok(())
}

async fn wait_with_probe(
    state: &Arc<AppState>,
    generation: u64,
    operation: Operation,
    probe: &mut impl DataPlaneProbe,
) -> AppResult<()> {
    let started = Instant::now();
    let mut consecutive_ready = 0u8;
    loop {
        sleep(PROBE_INTERVAL).await;
        check_process(state, generation, operation).await?;

        let tun_ready = probe.tun_ready();
        let clash_ready = if started.elapsed() >= operation.settle_time() {
            probe.clash_ready().await
        } else {
            false
        };
        if tun_ready && clash_ready {
            consecutive_ready += 1;
            if consecutive_ready >= 2 {
                if matches!(operation, Operation::Start) {
                    info!(
                        elapsed_ms = started.elapsed().as_millis(),
                        "sing-box data plane is ready"
                    );
                }
                return Ok(());
            }
        } else {
            consecutive_ready = 0;
        }

        if started.elapsed() >= READY_TIMEOUT {
            if matches!(operation, Operation::Start) {
                terminate_failed_start(state, generation).await;
            }
            return Err(AppError::message(operation.timeout()));
        }
    }
}

#[cfg(test)]
mod tests;
