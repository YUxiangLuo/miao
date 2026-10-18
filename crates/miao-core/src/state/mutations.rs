use std::{future::Future, sync::Arc};

use super::AppState;
use crate::error::{AppError, AppResult};

tokio::task_local! {
    static CURRENT_MUTATION: MutationGuard;
}

/// Clones share one registration. This lets an upgrade transfer its accepted
/// request to the delayed exec task without disappearing from shutdown's drain.
#[derive(Clone)]
pub(crate) struct MutationGuard(Arc<MutationLease>);
struct MutationLease {
    state: Arc<AppState>,
}

impl Drop for MutationLease {
    fn drop(&mut self) {
        self.state.lifecycle.finish_mutation();
    }
}

impl MutationGuard {
    pub(crate) async fn scope<F: Future>(self, future: F) -> F::Output {
        CURRENT_MUTATION.scope(self, future).await
    }
}

pub(crate) struct UpgradeQuiescence {
    state: Arc<AppState>,
    _mutation: MutationGuard,
}

impl Drop for UpgradeQuiescence {
    fn drop(&mut self) {
        // Shutdown remains terminal even when an upgrade fails or is cancelled.
        self.state.lifecycle.resume_mutations();
        self.state.scheduled_refresh_wake.notify_waiters();
    }
}

impl AppState {
    pub(crate) fn begin_mutation(self: &Arc<Self>) -> Option<MutationGuard> {
        self.lifecycle.register_mutation().then(|| {
            MutationGuard(Arc::new(MutationLease {
                state: self.clone(),
            }))
        })
    }

    /// No config_update lock may be held here: admitted SSH/network work and
    /// queued writes still need it. Exempt precisely this request's lease, not
    /// an arbitrary count of tasks. Standalone callers acquire their own lease.
    pub(crate) async fn quiesce_for_upgrade(self: &Arc<Self>) -> AppResult<UpgradeQuiescence> {
        let mutation = CURRENT_MUTATION
            .try_with(|guard| Arc::ptr_eq(&guard.0.state, self).then(|| guard.clone()))
            .ok()
            .flatten()
            .or_else(|| self.begin_mutation())
            .ok_or_else(|| AppError::message("Service is not accepting upgrades"))?;
        if !self.lifecycle.quiesce_mutations() {
            return Err(AppError::message(
                "Service is already shutting down or upgrading",
            ));
        }
        let quiescence = UpgradeQuiescence {
            state: self.clone(),
            _mutation: mutation,
        };
        tokio::select! {
            biased;
            _ = self.lifecycle.wait_for_shutdown() => {
                return Err(AppError::message("Upgrade cancelled by shutdown"));
            }
            _ = self.lifecycle.wait_for_mutations_except(1) => {}
        }
        Ok(quiescence)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[tokio::test]
    async fn upgrade_drains_other_writes_without_waiting_on_itself_or_holding_config_lock() {
        let (_root, state) = crate::test_support::isolated_stopped_state(Default::default());
        let other_write = state.begin_mutation().unwrap();
        let upgrade_request = state.begin_mutation().unwrap();
        let upgrading = state.clone();
        let task = tokio::spawn(
            upgrade_request.scope(async move { upgrading.quiesce_for_upgrade().await }),
        );
        tokio::time::timeout(Duration::from_secs(1), async {
            while state.lifecycle.mutations_allowed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(state.begin_mutation().is_none());
        assert!(!task.is_finished());
        let config_lock = state
            .config_update
            .try_lock()
            .expect("drain must not hold the transaction lock");
        drop(config_lock);
        drop(other_write);
        let quiescence = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(state.begin_mutation().is_none());
        // The HTTP request has returned; its lease still belongs to handoff.
        state.lifecycle.shutdown();
        assert!(state
            .lifecycle
            .with_upgrade_handoff(|| panic!("must not exec during shutdown"))
            .is_none());
        assert!(tokio::time::timeout(
            Duration::from_millis(20),
            state.lifecycle.wait_for_mutations()
        )
        .await
        .is_err());
        drop(quiescence);
        state.lifecycle.wait_for_mutations().await;
        assert!(state.begin_mutation().is_none());
    }

    #[tokio::test]
    async fn isolated_exec_happens_only_after_an_accepted_write_commits() {
        let root = tempfile::tempdir().unwrap();
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "state::mutations::tests::upgrade_exec_child",
                    "--nocapture",
                ])
                .env("MIAO_TEST_UPGRADE_HANDOFF_DIR", root.path())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .expect("isolated handoff must finish without self-deadlock")
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("exec-completed")).unwrap(),
            "complete"
        );
    }

    #[tokio::test]
    async fn upgrade_exec_child() {
        use crate::{models::Config, paths::RuntimePaths, state::AppState};
        use std::{os::unix::process::CommandExt, path::PathBuf, sync::Arc};
        let Some(directory) = std::env::var_os("MIAO_TEST_UPGRADE_HANDOFF_DIR") else {
            return;
        };
        let root = PathBuf::from(directory);
        let config = Config::default();
        let config_path = root.join("config.yaml");
        let state = Arc::new(
            AppState::with_config_layers(
                (&config).into(),
                config,
                config_path.clone(),
                root.join("volatile.yaml"),
                RuntimePaths::new(root.join("runtime"), &config_path),
            )
            .unwrap(),
        );
        let accepted = state.begin_mutation().unwrap();
        let writing = state.clone();
        let write_path = root.join("accepted-write");
        let writer = tokio::spawn(async move {
            let _accepted = accepted;
            // Model remote provisioning outside the config lock. The upgrade
            // must leave us time to perform the subsequent local commit.
            while writing.lifecycle.mutations_allowed() {
                tokio::task::yield_now().await;
            }
            assert!(writing.begin_mutation().is_none());
            let _commit = writing.config_update.lock().await;
            tokio::fs::write(write_path, "committed").await.unwrap();
        });
        let request = state.begin_mutation().unwrap();
        request.scope(async {
            let _quiescence = state.quiesce_for_upgrade().await.unwrap();
            writer.await.unwrap();
            let _commit = state.config_update.lock().await;
            // Only this isolated test subprocess is replaced. No application
            // executable, production config or actual service is touched.
            let result = state.lifecycle.with_upgrade_handoff(|| {
                std::process::Command::new("sh")
                    .args(["-c", "test -f \"$1/accepted-write\" && printf complete > \"$1/exec-completed\"", "handoff"])
                    .arg(&root).exec()
            });
            panic!("exec unexpectedly returned: {result:?}");
        }).await;
    }

    #[tokio::test]
    async fn cancelled_upgrade_reopens_admission_and_releases_its_standalone_lease() {
        let (_root, state) = crate::test_support::isolated_stopped_state(Default::default());
        let other_write = state.begin_mutation().unwrap();
        let upgrading = state.clone();
        let task = tokio::spawn(async move { upgrading.quiesce_for_upgrade().await });
        tokio::time::timeout(Duration::from_secs(1), async {
            while state.lifecycle.mutations_allowed() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(matches!(task.await, Err(err) if err.is_cancelled()));
        assert!(state.begin_mutation().is_some());
        drop(other_write);
        state.lifecycle.shutdown();
        state.lifecycle.wait_for_mutations().await;
    }
}
