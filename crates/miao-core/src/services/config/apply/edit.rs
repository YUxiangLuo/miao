use super::*;

/// Owns the read/modify/apply lock for a local edit. Validation that depends on
/// current nodes/rules belongs inside this scope. Dropping an uncommitted edit
/// only drops the candidate: no file or observable state has been changed.
/// Subscription edits use `edit_subscriptions` instead, with HTTP outside the lock.
pub(crate) struct ConfigEdit<'a> {
    state: &'a Arc<AppState>,
    _guard: tokio::sync::MutexGuard<'a, ()>,
    before: Config,
    pub candidate: Config,
}

impl<'a> ConfigEdit<'a> {
    pub async fn begin(state: &'a Arc<AppState>) -> Self {
        let guard = state.config_update.lock().await;
        let before = state.config.read().await.clone();
        Self {
            state,
            _guard: guard,
            candidate: before.clone(),
            before,
        }
    }

    pub fn original(&self) -> &Config {
        &self.before
    }

    pub async fn commit(self) -> AppResult<ConfigApplyEffect> {
        self.apply().await
    }

    // Preference transactions must retain the lock through preference rollback
    // and through observing effective state, not only through runtime apply.
    pub(super) async fn apply(&self) -> AppResult<ConfigApplyEffect> {
        if self.before.subs != self.candidate.subs {
            return Err(AppError::message(
                "Subscription edits require edit_subscriptions",
            ));
        }
        transaction::apply_config_change_with_source(
            self.state,
            &self.before,
            &self.candidate,
            SubSource::SnapshotOrLocal,
        )
        .await
    }
}
