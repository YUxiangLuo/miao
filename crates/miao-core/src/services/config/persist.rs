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
pub(super) async fn write_file_atomic(path: &Path, content: &[u8]) -> AppResult<()> {
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

    write_file_atomic(path, yaml.as_bytes()).await
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
    save_config_cache_at(
        &get_sing_box_home().join("config.json"),
        &config_cache_path(),
    )
    .await;
}

/// 原子保存：读当前 config.json 字节，经临时文件 rename 落 cache。
/// 进程在 copy 中途被杀不会留下半截 cache（回滚/启动快速通道会把它当好配置用）。
async fn save_config_cache_at(config_path: &Path, cache_path: &Path) {
    match tokio::fs::read(config_path).await {
        Ok(bytes) => match write_file_atomic(cache_path, &bytes).await {
            Ok(()) => info!(path = %cache_path.display(), "Config cache saved"),
            Err(e) => error!("Failed to save config cache: {}", e),
        },
        Err(e) => error!("Failed to read runtime config for cache: {}", e),
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
    restore_config_from_cache_at(
        &config_cache_path(),
        &get_sing_box_home().join("config.json"),
    )
    .await
}

async fn restore_config_from_cache_at(cache_path: &Path, config_path: &Path) -> AppResult<()> {
    if !cache_path.exists() {
        return Err(AppError::message("No cached config available"));
    }
    let bytes = tokio::fs::read(cache_path)
        .await
        .map_err(|e| AppError::context("Failed to read config cache", e))?;
    if bytes.is_empty() {
        return Err(AppError::message("Cached config is empty"));
    }
    write_file_atomic(config_path, &bytes).await?;
    info!(path = %cache_path.display(), "Restored config from cache");
    Ok(())
}

/// 配置变更前把当前 config.json 的字节读进内存：回滚 tier 1 材料。
/// 它正在跑/刚跑过，必然已知可用；文件不在、读不出或为空都视为没有快照。
pub async fn snapshot_runtime_config() -> Option<Vec<u8>> {
    snapshot_runtime_config_at(&get_sing_box_home().join("config.json")).await
}

async fn snapshot_runtime_config_at(path: &Path) -> Option<Vec<u8>> {
    tokio::fs::read(path)
        .await
        .ok()
        .filter(|bytes| !bytes.is_empty())
}

/// 把快照字节原子写回 config.json：回滚是纯本地文件操作，不碰网络。
pub async fn restore_runtime_config_bytes(bytes: &[u8]) -> AppResult<()> {
    restore_runtime_config_bytes_at(&get_sing_box_home().join("config.json"), bytes).await
}

async fn restore_runtime_config_bytes_at(path: &Path, bytes: &[u8]) -> AppResult<()> {
    write_file_atomic(path, bytes).await
}

/// 订阅节点集快照：上次真拉取拿到的节点，供本地语义变更（节点选择/路由模式/
/// 规则/去广告/手动节点）零网络重建配置。`subs` 是一致性护栏：与当前配置的
/// 订阅列表不一致时快照作废（订阅增删后必须真拉取一次重建快照）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SubNodesSnapshot {
    pub subs: Vec<String>,
    pub node_names: Vec<String>,
    pub outbounds: Vec<serde_json::Value>,
}

impl SubNodesSnapshot {
    /// 快照能否服务于当前订阅列表
    pub fn matches_subs(&self, subs: &[String]) -> bool {
        self.subs == subs
    }
}

pub fn sub_nodes_snapshot_path() -> PathBuf {
    get_sing_box_home().join("sub-nodes.json")
}

pub fn has_sub_nodes_snapshot() -> bool {
    sub_nodes_snapshot_path().exists()
}

pub async fn save_sub_nodes_snapshot(snapshot: &SubNodesSnapshot) -> AppResult<()> {
    save_sub_nodes_snapshot_at(&sub_nodes_snapshot_path(), snapshot).await
}

async fn save_sub_nodes_snapshot_at(path: &Path, snapshot: &SubNodesSnapshot) -> AppResult<()> {
    let bytes = serde_json::to_vec(snapshot)?;
    write_file_atomic(path, &bytes).await
}

/// 读取快照；文件缺失、读不出、内容损坏都视为没有快照（调用方退化到拉取路径）
pub async fn read_sub_nodes_snapshot() -> Option<SubNodesSnapshot> {
    read_sub_nodes_snapshot_at(&sub_nodes_snapshot_path()).await
}

async fn read_sub_nodes_snapshot_at(path: &Path) -> Option<SubNodesSnapshot> {
    let bytes = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        config_cache_path, get_sing_box_home, persist_effective_node_select,
        read_sub_nodes_snapshot_at, restore_config_from_cache_at, restore_runtime_config_bytes_at,
        save_config_cache_at, save_sub_nodes_snapshot_at, snapshot_runtime_config_at,
        SubNodesSnapshot,
    };

    #[test]
    fn config_cache_lives_under_sing_box_home() {
        assert_eq!(
            config_cache_path(),
            get_sing_box_home().join("config.json.cache")
        );
    }

    #[tokio::test]
    async fn snapshot_and_restore_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("miao-snapshot-{}", std::process::id()));
        let config_path = temp_dir.join("config.json");
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        // 不存在/空文件都没有快照
        assert!(snapshot_runtime_config_at(&config_path).await.is_none());
        tokio::fs::write(&config_path, b"").await.unwrap();
        assert!(snapshot_runtime_config_at(&config_path).await.is_none());

        tokio::fs::write(&config_path, br#"{"v":1}"#).await.unwrap();
        let snapshot = snapshot_runtime_config_at(&config_path).await.unwrap();

        // 模拟 apply 覆盖成新配置，回滚写回应恢复旧字节
        tokio::fs::write(&config_path, br#"{"v":2}"#).await.unwrap();
        restore_runtime_config_bytes_at(&config_path, &snapshot)
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&config_path).await.unwrap(), br#"{"v":1}"#);
        // 原子写不留下临时文件
        assert!(!temp_dir.join("config.tmp").exists());

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn cache_save_and_restore_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("miao-cache-{}", std::process::id()));
        let config_path = temp_dir.join("config.json");
        let cache_path = temp_dir.join("config.json.cache");
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        tokio::fs::write(&config_path, b"good").await.unwrap();
        save_config_cache_at(&config_path, &cache_path).await;
        assert_eq!(tokio::fs::read(&cache_path).await.unwrap(), b"good");
        assert!(!temp_dir.join("config.json.tmp").exists());

        tokio::fs::write(&config_path, b"bad").await.unwrap();
        restore_config_from_cache_at(&cache_path, &config_path)
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&config_path).await.unwrap(), b"good");

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn empty_or_missing_cache_is_rejected() {
        let temp_dir =
            std::env::temp_dir().join(format!("miao-cache-empty-{}", std::process::id()));
        let config_path = temp_dir.join("config.json");
        let cache_path = temp_dir.join("config.json.cache");
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        assert!(restore_config_from_cache_at(&cache_path, &config_path)
            .await
            .is_err());
        tokio::fs::write(&cache_path, b"").await.unwrap();
        assert!(restore_config_from_cache_at(&cache_path, &config_path)
            .await
            .is_err());

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn sub_nodes_snapshot_roundtrip_and_guards() {
        let temp_dir =
            std::env::temp_dir().join(format!("miao-sub-snapshot-{}", std::process::id()));
        let snapshot_path = temp_dir.join("sub-nodes.json");
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        // 缺失/损坏都读不出
        assert!(read_sub_nodes_snapshot_at(&snapshot_path).await.is_none());
        tokio::fs::write(&snapshot_path, b"not-json").await.unwrap();
        assert!(read_sub_nodes_snapshot_at(&snapshot_path).await.is_none());

        let snapshot = SubNodesSnapshot {
            subs: vec!["https://a.example.com".to_string()],
            node_names: vec!["香港 01".to_string()],
            outbounds: vec![serde_json::json!({"type": "trojan", "tag": "香港 01"})],
        };
        save_sub_nodes_snapshot_at(&snapshot_path, &snapshot)
            .await
            .unwrap();
        assert!(!temp_dir.join("sub-nodes.tmp").exists());

        let loaded = read_sub_nodes_snapshot_at(&snapshot_path).await.unwrap();
        assert!(loaded.matches_subs(&["https://a.example.com".to_string()]));
        assert!(!loaded.matches_subs(&["https://b.example.com".to_string()]));
        assert!(!loaded.matches_subs(&[]));
        assert_eq!(loaded.node_names, vec!["香港 01".to_string()]);

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
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
