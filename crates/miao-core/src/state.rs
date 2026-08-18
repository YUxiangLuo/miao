use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

use crate::models::{Config, GitHubRelease, SubStatus};

/// 应用状态容器 - 包含所有运行时状态
/// 通过依赖注入传递，避免全局静态变量
pub struct AppState {
    pub config: RwLock<Config>, // 使用 RwLock 支持并发读
    pub config_path: PathBuf,
    /// 易变层配置（node_select/route_mode）的落盘位置，与 config_path 分层。
    pub volatile_path: PathBuf,
    pub config_update: Mutex<()>,
    pub sing_process: Mutex<Option<SingBoxProcess>>,
    /// 每次有意启动/停止 sing-box 都会递增。崩溃看门狗以此识别自己监护的
    /// 那次启动是否已被取代，避免与配置热重启、用户停核等路径打架。
    pub sing_generation: AtomicU64,
    pub sub_status: Mutex<HashMap<String, SubStatus>>,
    pub config_warning: Mutex<Option<String>>,
    /// 最近一次生成配置时因出口节点不存在而被跳过的自定义规则,用于面板告警与规则列表标记
    pub skipped_rules: Mutex<Vec<SkippedRule>>,
    pub initializing: AtomicBool,
    /// Desired service state. It remains true during onboarding so the first
    /// valid configuration starts sing-box, and becomes false after an
    /// explicit stop request so later config edits do not restart it.
    pub service_should_run: AtomicBool,
    pub http_client: reqwest::Client,
    pub version_cache: ArcSwap<VersionCache>, // 使用 ArcSwap 实现无锁读取
    pub upgrading: AtomicBool,                // 防止并发升级
}

impl AppState {
    /// 创建新的应用状态实例
    #[cfg(test)]
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        Self::with_config_path(
            config,
            PathBuf::from("config.yaml"),
            PathBuf::from("volatile.yaml"),
        )
    }

    pub fn with_config_path(
        config: Config,
        config_path: PathBuf,
        volatile_path: PathBuf,
    ) -> Result<Self, reqwest::Error> {
        // reqwest 默认会读 HTTP_PROXY/HTTPS_PROXY 等环境变量代理。本进程自己
        // 就是代理：订阅拉取、Clash API（127.0.0.1）都不该被 root 环境里的
        // 代理变量劫持，显式禁用。
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .no_proxy()
            .build()?;

        Ok(Self {
            config: RwLock::new(config),
            config_path,
            volatile_path,
            config_update: Mutex::new(()),
            sing_process: Mutex::new(None),
            sing_generation: AtomicU64::new(0),
            sub_status: Mutex::new(HashMap::new()),
            config_warning: Mutex::new(None),
            skipped_rules: Mutex::new(Vec::new()),
            initializing: AtomicBool::new(true),
            service_should_run: AtomicBool::new(true),
            http_client,
            version_cache: ArcSwap::new(Arc::new(VersionCache {
                release: None,
                fetched_at: None,
            })),
            upgrading: AtomicBool::new(false),
        })
    }
}

/// 因出口节点不存在而在生成配置时被跳过的自定义规则
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedRule {
    /// 配置文件中的规则原文,用于与规则列表条目对应
    pub raw: String,
    /// 人类可读的失效描述,用于状态告警
    pub description: String,
}

pub struct SingBoxProcess {
    pub child: tokio::process::Child,
    pub started_at: Instant,
}

/// 版本信息缓存
#[derive(Clone)]
pub struct VersionCache {
    pub release: Option<GitHubRelease>,
    pub fetched_at: Option<Instant>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_state_new_creates_valid_instance() {
        let config = Config {
            port: Some(8080),
            subs: vec!["https://example.com/sub".to_string()],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
            adblock: false,
            mcp: false,
            node_select: Default::default(),
        };

        let state = AppState::new(config.clone()).unwrap();

        // 验证状态正确初始化
        assert!(state
            .initializing
            .load(std::sync::atomic::Ordering::Relaxed));
        assert!(state
            .service_should_run
            .load(std::sync::atomic::Ordering::Relaxed));

        // 验证配置被正确存储
        let locked_config = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async { state.config.read().await.clone() });
        assert_eq!(locked_config.port, Some(8080));
        assert_eq!(locked_config.subs.len(), 1);
        assert_eq!(state.config_path, PathBuf::from("config.yaml"));
    }

    #[test]
    fn version_cache_starts_empty() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
            adblock: false,
            mcp: false,
            node_select: Default::default(),
        };

        let state = AppState::new(config).unwrap();
        let cache = state.version_cache.load();

        assert!(cache.release.is_none());
        assert!(cache.fetched_at.is_none());
    }
}
