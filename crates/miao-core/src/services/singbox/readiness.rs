use std::future::Future;

use super::*;
use tokio::time::Instant;

const PROBE_INTERVAL: Duration = Duration::from_millis(25);
const CLASH_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
// OpenWrt can need several seconds to install auto_redirect and bind Clash.
const READY_TIMEOUT: Duration = Duration::from_secs(8);
const REQUIRED_CONSECUTIVE_READY: u8 = 2;

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Start,
    #[cfg(unix)]
    Reload {
        pid: u32,
    },
}

impl Operation {
    fn settle_time(self) -> Duration {
        match self {
            Self::Start => Duration::from_millis(100),
            #[cfg(unix)]
            Self::Reload { .. } => Duration::from_millis(500),
        }
    }

    fn cancelled(self) -> &'static str {
        match self {
            Self::Start => "sing-box start was cancelled",
            #[cfg(unix)]
            Self::Reload { .. } => "sing-box reload was cancelled",
        }
    }

    fn missing_process_error(self) -> &'static str {
        match self {
            Self::Start => self.cancelled(),
            #[cfg(unix)]
            Self::Reload { .. } => "sing-box exited during reload",
        }
    }

    fn status_check_context(self) -> &'static str {
        match self {
            Self::Start => "Failed to check sing-box startup status",
            #[cfg(unix)]
            Self::Reload { .. } => "Failed to check sing-box reload status",
        }
    }

    fn phase_label(self) -> &'static str {
        match self {
            Self::Start => "startup",
            #[cfg(unix)]
            Self::Reload { .. } => "reload",
        }
    }

    fn timeout(self) -> &'static str {
        match self {
            Self::Start => {
                "sing-box process started but its data plane did not become ready within 8 seconds"
            }
            #[cfg(unix)]
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
    activation: &activation::Activation,
) -> AppResult<()> {
    #[cfg(not(test))]
    let mut probe = SystemProbe {
        client: &state.http_client,
        clash_url: clash_api_url("/version"),
    };
    #[cfg(test)]
    let mut probe = tests::ReadyProbe;

    wait_with_activation(state, generation, operation, &mut probe, Some(activation)).await
}

async fn check_process(state: &AppState, generation: u64, operation: Operation) -> AppResult<u32> {
    let mut slot = state.sing_process.lock().await;
    if !start_still_current(state, generation) {
        return Err(AppError::message(operation.cancelled()));
    }
    let process = slot
        .as_mut()
        .ok_or_else(|| AppError::message(operation.missing_process_error()))?;

    #[cfg(unix)]
    if let Operation::Reload { pid } = operation {
        if process.child.id() != Some(pid) {
            return Err(AppError::message(
                "sing-box process changed unexpectedly during reload",
            ));
        }
    }
    if let Some(status) = process
        .child
        .try_wait()
        .map_err(|e| AppError::context(operation.status_check_context(), e))?
    {
        *slot = None;
        #[cfg(windows)]
        super::cleanup_stale_tun_adapter();
        return Err(AppError::message(format!(
            "sing-box exited during {} with code {}",
            operation.phase_label(),
            status.code().unwrap_or(-1)
        )));
    }
    slot.as_ref()
        .and_then(|process| process.child.id())
        .ok_or_else(|| AppError::message(operation.cancelled()))
}

#[cfg(all(test, unix))]
async fn wait_with_probe(
    state: &Arc<AppState>,
    generation: u64,
    operation: Operation,
    probe: &mut impl DataPlaneProbe,
) -> AppResult<()> {
    wait_with_activation(state, generation, operation, probe, None).await
}

async fn wait_with_activation(
    state: &Arc<AppState>,
    generation: u64,
    operation: Operation,
    probe: &mut impl DataPlaneProbe,
    activation: Option<&activation::Activation>,
) -> AppResult<()> {
    let started = Instant::now();
    let mut consecutive_ready = 0u8;
    loop {
        sleep(PROBE_INTERVAL).await;
        let pid = check_process(state, generation, operation).await?;
        let acknowledged = match activation {
            Some(activation) => activation.acknowledged(pid).await,
            None => true, // Probe-loop unit tests only; controller always supplies a request.
        };

        let tun_ready = probe.tun_ready();
        let clash_ready = if started.elapsed() >= operation.settle_time() {
            probe.clash_ready().await
        } else {
            false
        };
        if acknowledged && tun_ready && clash_ready {
            consecutive_ready += 1;
            if consecutive_ready >= REQUIRED_CONSECUTIVE_READY {
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
