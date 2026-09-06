use std::future::Future;

use super::*;

/// Only the field/path differ. Acceptance, activation, and rollback must not
/// diverge between requested strategy and requested multiplier.
trait RuntimePreference: Copy + Send {
    const PERSIST_LABEL: &'static str;
    const APPLY_LABEL: &'static str;
    fn path(state: &AppState) -> &Path;
    fn current(state: &AppState) -> impl Future<Output = Self> + Send;
    fn stage(self, state: &AppState) -> impl Future<Output = ()> + Send;
    fn persist(self, state: &Arc<AppState>) -> impl Future<Output = AppResult<()>> + Send;
    fn update(self, config: &mut Config);
}

impl RuntimePreference for NodeSelect {
    const PERSIST_LABEL: &'static str = "node-selection strategy";
    const APPLY_LABEL: &'static str = "node-selection strategy";
    fn path(state: &AppState) -> &Path {
        &state.runtime_paths.node_select_preference
    }
    async fn current(state: &AppState) -> Self {
        *state.node_select_preference.read().await
    }
    async fn stage(self, state: &AppState) {
        *state.node_select_preference.write().await = self;
    }
    async fn persist(self, state: &Arc<AppState>) -> AppResult<()> {
        save_node_select_preference(state, self).await
    }
    fn update(self, config: &mut Config) {
        config.node_select = self;
    }
}

impl RuntimePreference for Option<NodeMultiplier> {
    const PERSIST_LABEL: &'static str = "max-multiplier preference";
    const APPLY_LABEL: &'static str = "max multiplier";
    fn path(state: &AppState) -> &Path {
        &state.runtime_paths.max_multiplier_preference
    }
    async fn current(state: &AppState) -> Self {
        *state.max_multiplier_preference.read().await
    }
    async fn stage(self, state: &AppState) {
        *state.max_multiplier_preference.write().await = self;
    }
    async fn persist(self, state: &Arc<AppState>) -> AppResult<()> {
        save_max_multiplier_preference(state, self).await
    }
    fn update(self, config: &mut Config) {
        config.max_multiplier = self;
    }
}

async fn preference_failure(
    path: &Path,
    snapshot: Option<&[u8]>,
    context: &str,
    cause: AppError,
) -> ConfigMutationError {
    ConfigMutationError::Apply(match restore_file_snapshot(path, snapshot).await {
        Ok(()) => cause,
        Err(rollback) => AppError::message(format!(
            "{context}: {cause}. Preference rollback failed: {rollback}"
        )),
    })
}

async fn apply_preference<P: RuntimePreference>(
    state: &Arc<AppState>,
    requested: P,
) -> Result<(P, NodeSelect, RuntimeUpdate), ConfigMutationError> {
    let mut edit = ConfigEdit::begin(state).await;
    let previous = P::current(state).await;
    let path = P::path(state);
    // Reject unreadable rollback material before changing any preference/runtime.
    let snapshot = read_file_snapshot(path)
        .await
        .map_err(ConfigMutationError::Apply)?;
    if let Err(error) = requested.persist(state).await {
        return Err(preference_failure(
            path,
            snapshot.as_deref(),
            &format!("Failed to persist {}", P::PERSIST_LABEL),
            error,
        )
        .await);
    }
    // Requested preferences are staged under config_update because generation
    // overlays them onto the effective candidate. A failed activation restores
    // both the in-memory requested value and its exact prior file bytes.
    requested.stage(state).await;
    requested.update(&mut edit.candidate);
    let result = if edit.candidate == *edit.original() {
        Ok(RuntimeUpdate::None)
    } else {
        // Keep the edit guard until effective state is observed below.
        edit.apply().await.map(|effect| effect.runtime_update())
    };
    match result {
        Ok(update) => Ok((previous, state.config.read().await.node_select, update)),
        Err(error) => {
            previous.stage(state).await;
            Err(preference_failure(
                path,
                snapshot.as_deref(),
                &format!("Failed to apply {}", P::APPLY_LABEL),
                error,
            )
            .await)
        }
    }
}

/// Caller must not hold config_update. Effective may be manual after fallback;
/// all return values are observed before releasing the transaction lock.
pub async fn apply_node_select(
    state: &Arc<AppState>,
    requested: NodeSelect,
) -> Result<(NodeSelect, NodeSelect, RuntimeUpdate), ConfigMutationError> {
    apply_preference(state, requested).await
}

pub async fn apply_max_multiplier(
    state: &Arc<AppState>,
    requested: Option<NodeMultiplier>,
) -> Result<(Option<NodeMultiplier>, RuntimeUpdate), ConfigMutationError> {
    apply_preference(state, requested)
        .await
        .map(|(previous, _, update)| (previous, update))
}
