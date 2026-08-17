use std::path::{Path, PathBuf};
use tracing::{error, info};

use std::sync::Arc;

use crate::error::{AppError, AppResult};
use crate::models::{Config, NodeSelect};
use crate::services::singbox::get_sing_box_home;
use crate::state::AppState;

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

/// 筛空地区后把 yaml / 内存里的 node_select 写回 manual。
pub async fn persist_effective_node_select(
    state: &Arc<AppState>,
    node_select: NodeSelect,
) -> AppResult<()> {
    let mut config = state.config.read().await.clone();
    if config.node_select == node_select {
        return Ok(());
    }
    config.node_select = node_select;
    save_config_to(&state.config_path, &config).await?;
    *state.config.write().await = config;
    Ok(())
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

/// 上次成功运行的生成配置缓存是否存在（启动快速通道的入场券）
pub fn has_config_cache() -> bool {
    config_cache_path().exists()
}

/// 读取缓存内容，用于订阅刷新后的变更比对
pub async fn read_config_cache() -> Option<Vec<u8>> {
    tokio::fs::read(config_cache_path()).await.ok()
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
    use super::{config_cache_path, get_sing_box_home, persist_effective_node_select};

    #[test]
    fn config_cache_lives_under_sing_box_home() {
        assert_eq!(
            config_cache_path(),
            get_sing_box_home().join("config.json.cache")
        );
    }

    #[tokio::test]
    async fn persist_effective_node_select_writes_manual_fallback() {
        use crate::models::{Config, NodeSelect, Region};
        use crate::test_support::app_state;

        let state = app_state(Config {
            node_select: NodeSelect::Fastest(Region::Hk),
            ..Config::default()
        });
        persist_effective_node_select(&state, NodeSelect::Manual)
            .await
            .unwrap();

        assert_eq!(state.config.read().await.node_select, NodeSelect::Manual);
        let yaml = tokio::fs::read_to_string(&state.config_path).await.unwrap();
        assert!(!yaml.contains("node_select"));
        let _ = tokio::fs::remove_file(&state.config_path).await;
    }
}
