use std::path::{Path, PathBuf};
use tracing::{error, info};

use crate::error::{AppError, AppResult};
use crate::models::Config;
use crate::services::singbox::get_sing_box_home;

pub(super) fn config_cache_path() -> PathBuf {
    get_sing_box_home().join("config.json.cache")
}

/// 原子写入文件：先写入临时文件，再重命名为目标文件
pub(super) async fn write_file_atomic(path: &Path, content: &str) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::context("Failed to create config directory", e))?;
    }

    let temp_path = path.with_extension("tmp");

    // 先写入临时文件
    tokio::fs::write(&temp_path, content)
        .await
        .map_err(|e| AppError::context("Failed to write temp file", e))?;

    // 原子重命名为最终文件
    tokio::fs::rename(&temp_path, path)
        .await
        .map_err(|e| AppError::context("Failed to atomically rename file", e))?;

    Ok(())
}

pub async fn save_config_to(path: &Path, config: &Config) -> AppResult<()> {
    let yaml = serde_yaml::to_string(config)?;
    if let Ok(existing) = tokio::fs::read_to_string(path).await {
        if existing == yaml {
            info!(config_path = ?path, "Config file already up to date, skipping write");
            return Ok(());
        }
    }

    write_file_atomic(path, &yaml).await
}

pub async fn save_config_cache() {
    let cache_path = config_cache_path();
    if let Some(parent) = cache_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            error!("Failed to create config cache directory: {}", e);
            return;
        }
    }

    let config_path = get_sing_box_home().join("config.json");
    if let Err(e) = tokio::fs::copy(&config_path, &cache_path).await {
        error!("Failed to save config cache: {}", e);
    } else {
        info!(path = %cache_path.display(), "Config cache saved");
    }
}

pub async fn restore_config_from_cache() -> AppResult<()> {
    let cache_path = config_cache_path();
    if !cache_path.exists() {
        return Err(AppError::message("No cached config available"));
    }
    let config_path = get_sing_box_home().join("config.json");
    tokio::fs::copy(&cache_path, &config_path)
        .await
        .map_err(|e| AppError::context("Failed to restore config from cache", e))?;
    info!(path = %cache_path.display(), "Restored config from cache");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{config_cache_path, get_sing_box_home};

    #[test]
    fn config_cache_lives_under_sing_box_home() {
        assert_eq!(
            config_cache_path(),
            get_sing_box_home().join("config.json.cache")
        );
    }
}
