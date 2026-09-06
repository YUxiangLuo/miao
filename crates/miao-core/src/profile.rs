//! Resolve one launch's file ownership once, before reading or writing state.
//! SDK launches never inspect the host process's command-line arguments.
use std::path::{Path, PathBuf};

use crate::{
    error::{AppError, AppResult},
    paths::{self, ConfigPathResolution, ConfigPathSource, RuntimePaths},
    runtime::RuntimeOptions,
};

mod platform;
use platform::PreferenceStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProfileKind {
    Default,
    Named,
    Temporary,
}

pub(crate) struct ResolvedProfile {
    pub kind: ProfileKind,
    pub config: ConfigPathResolution,
    pub runtime: RuntimePaths,
    pub volatile: PathBuf,
    pub log: Option<PathBuf>,
    pub temporary: Option<tempfile::TempDir>,
    legacy_bindings: Option<PathBuf>,
}

/// Captured environment makes path policy testable without changing cwd,
/// process arguments, platform directories, or environment variables.
struct Environment {
    default_config: ConfigPathResolution,
    runtime_root: PathBuf,
    data_dir: PathBuf,
    cwd: PathBuf,
    windows: bool,
    preferences: PreferenceStore,
}

impl Environment {
    fn current() -> AppResult<Self> {
        Ok(Self {
            default_config: paths::resolve_default_config_path(),
            runtime_root: crate::services::singbox::get_sing_box_home(),
            data_dir: paths::platform_data_dir(),
            cwd: std::env::current_dir()?,
            windows: cfg!(windows),
            preferences: platform::preference_store(),
        })
    }
}

/// Resolve existing symlinks and relative aliases. For a new configuration,
/// resolve its nearest existing ancestor so its identity survives first save.
fn resolve_path(path: &Path, cwd: &Path) -> AppResult<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    };
    let mut ancestor = absolute.as_path();
    let mut tail = Vec::new();
    loop {
        match std::fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                for component in tail.into_iter().rev() {
                    if component == ".." {
                        resolved.pop();
                    } else if component != "." {
                        resolved.push(component);
                    }
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let component = ancestor.components().next_back().ok_or_else(|| {
                    AppError::message("Configuration path has no existing ancestor")
                })?;
                tail.push(component.as_os_str().to_owned());
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| AppError::message("Invalid profile path"))?;
            }
            Err(error) => return Err(AppError::context("Failed to resolve profile path", error)),
        }
    }
}

impl ResolvedProfile {
    pub fn resolve(
        options: &RuntimeOptions,
        temporary: Option<tempfile::TempDir>,
    ) -> AppResult<Self> {
        Self::resolve_in(options, temporary, &Environment::current()?)
    }

    fn resolve_in(
        options: &RuntimeOptions,
        temporary: Option<tempfile::TempDir>,
        env: &Environment,
    ) -> AppResult<Self> {
        let config_path = resolve_path(
            options
                .config_path
                .as_deref()
                .unwrap_or(&env.default_config.path),
            &env.cwd,
        )?;
        // An inaccessible *unselected* installed profile must never prevent a
        // temporary/explicit launch. Only resolve it for best-effort identity
        // comparison; its contents and legacy state are never imported.
        let is_default = options.config_path.is_none()
            || (temporary.is_none()
                && (config_path == env.default_config.path
                    || resolve_path(&env.default_config.path, &env.cwd)
                        .ok()
                        .as_ref()
                        == Some(&config_path)));
        let kind = if temporary.is_some() {
            ProfileKind::Temporary
        } else if is_default {
            ProfileKind::Default
        } else {
            ProfileKind::Named
        };
        let id = paths::profile_id(&config_path);
        let persistent = paths::profile_state_dir(&config_path);
        let runtime_dir = match &options.runtime_dir {
            Some(path) => resolve_path(path, &env.cwd)?,
            None if kind == ProfileKind::Named => env.runtime_root.join("profiles").join(&id),
            None => env.runtime_root.clone(),
        };
        let overridden = options.runtime_dir.is_some();
        let preference_dir = if overridden || env.preferences == PreferenceStore::Runtime {
            runtime_dir.clone()
        } else if kind == ProfileKind::Named {
            persistent.clone()
        } else {
            match env.preferences {
                PreferenceStore::Working => env.cwd.clone(),
                PreferenceStore::Persistent => env.data_dir.clone(),
                PreferenceStore::Runtime => unreachable!("handled above"),
            }
        };
        let volatile = match &options.volatile_path {
            Some(path) => resolve_path(path, &env.cwd)?,
            None if overridden || !env.windows => runtime_dir.join("volatile.yaml"),
            None if kind == ProfileKind::Named => persistent.join("volatile.yaml"),
            None => env.data_dir.join("volatile.yaml"),
        };
        let log = match &options.log_path {
            Some(path) => Some(resolve_path(path, &env.cwd)?),
            None if env.windows && overridden => Some(runtime_dir.join("miao.log")),
            // The installed desktop's log is process-scoped (tray "open log").
            None if env.windows => Some(env.data_dir.join("miao.log")),
            None => None,
        };
        let runtime = RuntimePaths::new(runtime_dir, &config_path).with_preferences(
            preference_dir.join(".last_proxy"),
            preference_dir.join(".node_select"),
            preference_dir.join(".max_multiplier"),
        );
        if let Some(owner) = &temporary {
            let root = std::fs::canonicalize(owner.path())?;
            if [
                &config_path,
                &runtime.runtime_dir,
                &runtime.node_bindings,
                &runtime.last_proxy,
                &runtime.node_select_preference,
                &runtime.max_multiplier_preference,
                &volatile,
            ]
            .iter()
            .any(|path| !path.starts_with(&root))
                || log.as_ref().is_some_and(|path| !path.starts_with(&root))
            {
                return Err(AppError::message(
                    "Temporary profile paths must remain inside its directory",
                ));
            }
        }
        let legacy = config_path.with_extension("node-bindings.json");
        let legacy_bindings = (!paths::is_standard_config(&config_path)
            && legacy != runtime.node_bindings)
            .then_some(legacy);
        Ok(Self {
            kind,
            config: ConfigPathResolution {
                path: config_path,
                source: if options.config_path.is_some() {
                    ConfigPathSource::Explicit
                } else {
                    env.default_config.source.clone()
                },
            },
            runtime,
            volatile,
            log,
            temporary,
            legacy_bindings,
        })
    }

    /// Only old per-config tag bindings are attributable enough to migrate.
    /// Never import shared preferences/volatile/cache into a named profile.
    /// Leave the old file intact; subsequent writes belong to the new path.
    pub async fn migrate_bindings(&self) -> AppResult<()> {
        let Some(legacy) = &self.legacy_bindings else {
            return Ok(());
        };
        match tokio::fs::metadata(&self.runtime.node_bindings).await {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        match tokio::fs::read(legacy).await {
            Ok(bytes) => {
                crate::services::config::write_file_atomic(&self.runtime.node_bindings, &bytes)
                    .await
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::context(
                "Failed to migrate profile node bindings",
                error,
            )),
        }
    }
}

#[cfg(test)]
mod tests;
