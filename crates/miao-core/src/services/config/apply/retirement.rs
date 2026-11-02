//! Retire generated runtime files before committing a configuration with no
//! runtime. All callers hold config_update; the running kernel is untouched
//! until persistence succeeds. Backups never occupy a path used for startup.
use std::path::{Path, PathBuf};

use crate::{
    error::{AppError, AppResult},
    paths::RuntimePaths,
};

#[derive(Default)]
pub(super) struct RuntimeRetirement {
    directory: Option<PathBuf>,
    moved: Vec<(PathBuf, PathBuf)>,
}

impl RuntimeRetirement {
    pub(super) async fn stage(paths: &RuntimePaths) -> AppResult<Self> {
        let mut retirement = Self::default();
        for path in [
            &paths.active_config,
            &paths.config_cache,
            &paths.cache_manifest,
        ] {
            if let Err(err) = retirement.move_file(&paths.runtime_dir, path).await {
                return match retirement.restore().await {
                    Ok(()) => Err(err),
                    Err(restore_err) => Err(AppError::message(format!(
                        "{err}. Runtime files rollback failed: {restore_err}"
                    ))),
                };
            }
        }
        Ok(retirement)
    }

    async fn move_file(&mut self, runtime_dir: &Path, path: &Path) -> AppResult<()> {
        let metadata = match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(AppError::context(
                    format!("Failed to inspect runtime file {}", path.display()),
                    err,
                ))
            }
        };
        // Never move/delete an unexpected directory and its contents.
        if !metadata.is_file() && !metadata.file_type().is_symlink() {
            return Err(AppError::message(format!(
                "Cannot retire non-file runtime path {}",
                path.display()
            )));
        }
        if self.directory.is_none() {
            // Keep on cancellation too: only an explicit commit discards the
            // backups. A failed rollback reports their location for recovery.
            self.directory = Some(
                tempfile::Builder::new()
                    .prefix(".retired-runtime-")
                    .tempdir_in(runtime_dir)
                    .map_err(|err| {
                        AppError::context("Failed to prepare runtime file retirement", err)
                    })?
                    .keep(),
            );
        }
        let backup = self
            .directory
            .as_ref()
            .unwrap()
            .join(self.moved.len().to_string());
        tokio::fs::rename(path, &backup).await.map_err(|err| {
            AppError::context(
                format!("Failed to retire runtime file {}", path.display()),
                err,
            )
        })?;
        self.moved.push((path.to_path_buf(), backup));
        Ok(())
    }

    pub(super) async fn restore(self) -> AppResult<()> {
        let mut errors = Vec::new();
        for (path, backup) in self.moved.iter().rev() {
            if let Err(err) = tokio::fs::rename(backup, path).await {
                errors.push(format!("{}: {err}", path.display()));
            }
        }
        if !errors.is_empty() {
            return Err(AppError::message(format!(
                "{}; retained backups at {}",
                errors.join("; "),
                self.directory.as_ref().unwrap().display()
            )));
        }
        self.discard().await;
        Ok(())
    }

    pub(super) async fn discard(self) {
        if let Some(directory) = self.directory {
            if let Err(err) = tokio::fs::remove_dir_all(&directory).await {
                tracing::warn!(path = %directory.display(), error = %err, "Failed to remove retired runtime backups; they cannot be used for startup");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_rollback_retains_backup_without_removing_unexpected_contents() {
        let (_root, state) = crate::test_support::isolated_stopped_state(Default::default());
        let active = &state.runtime_paths.active_config;
        tokio::fs::write(active, b"previous runtime").await.unwrap();
        let staged = RuntimeRetirement::stage(&state.runtime_paths)
            .await
            .unwrap();
        let directory = staged.directory.clone().unwrap();
        let backup = staged.moved[0].1.clone();
        tokio::fs::create_dir(active).await.unwrap();
        tokio::fs::write(active.join("keep"), b"unexpected contents")
            .await
            .unwrap();
        let error = staged.restore().await.unwrap_err().to_string();
        assert!(error.contains(&directory.display().to_string()));
        assert_eq!(tokio::fs::read(backup).await.unwrap(), b"previous runtime");
        assert_eq!(
            tokio::fs::read(active.join("keep")).await.unwrap(),
            b"unexpected contents"
        );
    }
}
