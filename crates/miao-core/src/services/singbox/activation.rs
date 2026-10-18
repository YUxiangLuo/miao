//! A healthy old TUN/Clash listener is not an acknowledgement of a reload.
//! The child captures a fresh request before reading config.json and publishes
//! it only after the corresponding instance has completed Start().
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{error::AppResult, state::AppState};

pub(super) const DIRECTORY_ENV: &str = "MIAO_ACTIVATION_DIR";
const REQUEST_FILE: &str = "activation-request.json";
const ACK_FILE: &str = "activation-ack.json";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Request {
    version: u32,
    request_id: String,
    config_sha256: String,
}

#[derive(Deserialize)]
struct Acknowledgement {
    #[serde(flatten)]
    request: Request,
    pid: u32,
}

pub(super) struct Activation {
    request: Request,
    ack_path: PathBuf,
}

impl Activation {
    /// Called while owning the process slot, before spawn or SIGHUP. Tokens
    /// distinguish repeated activations of identical bytes and watchdog restarts.
    pub(super) async fn prepare(state: &AppState) -> AppResult<Self> {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let bytes = tokio::fs::read(&state.runtime_paths.active_config).await?;
        let request = Request {
            version: 1,
            request_id: format!(
                "{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ),
            config_sha256: hex::encode(Sha256::digest(&bytes)),
        };
        crate::services::config::write_file_atomic(
            &state.runtime_paths.runtime_dir.join(REQUEST_FILE),
            &serde_json::to_vec(&request)?,
        )
        .await?;
        Ok(Self {
            request,
            ack_path: state.runtime_paths.runtime_dir.join(ACK_FILE),
        })
    }

    pub(super) async fn acknowledged(&self, pid: u32) -> bool {
        let Ok(bytes) = tokio::fs::read(&self.ack_path).await else {
            return false;
        };
        serde_json::from_slice::<Acknowledgement>(&bytes)
            .is_ok_and(|ack| ack.pid == pid && ack.request == self.request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acknowledgement_requires_exact_request_hash_and_child_pid() {
        let (_root, state) = crate::test_support::isolated_stopped_state(Default::default());
        std::fs::write(&state.runtime_paths.active_config, "{}").unwrap();
        let first = Activation::prepare(&state).await.unwrap();
        let second = Activation::prepare(&state).await.unwrap();
        assert_ne!(first.request.request_id, second.request.request_id);
        assert!(!second.acknowledged(42).await);
        let mut ack = serde_json::to_value(&first.request).unwrap();
        ack["pid"] = 42.into();
        std::fs::write(&first.ack_path, serde_json::to_vec(&ack).unwrap()).unwrap();
        assert!(first.acknowledged(42).await);
        assert!(!second.acknowledged(42).await);
        assert!(!first.acknowledged(43).await);
        ack["config_sha256"] = "wrong".into();
        std::fs::write(&first.ack_path, serde_json::to_vec(&ack).unwrap()).unwrap();
        assert!(!first.acknowledged(42).await);
        std::fs::write(&first.ack_path, "partial JSON").unwrap();
        assert!(!first.acknowledged(42).await);
    }
}
