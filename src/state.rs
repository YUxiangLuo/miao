use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

use crate::models::{Config, GitHubRelease, RouteMode, SubStatus};
use crate::services::geoip::{GeoIp, GeoPoint};

/// 应用状态容器 - 包含所有运行时状态
/// 通过依赖注入传递，避免全局静态变量
/// Clash 控制面的默认地址(sing-box 外部控制器)
pub const DEFAULT_CLASH_API_BASE: &str = "http://127.0.0.1:6262";

pub struct AppState {
    pub config: RwLock<Config>, // 使用 RwLock 支持并发读
    pub route_mode_override: RwLock<Option<RouteMode>>,
    pub config_path: PathBuf,
    pub clash_api_base: String,
    pub config_update: Mutex<()>,
    pub sing_process: Mutex<Option<SingBoxProcess>>,
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
    /// IP 地理定位数据库(内嵌 mmdb,启动时释放);库不可用时所有查询返回 None
    pub geo: GeoIp,
    /// 地图模式的本机/代理位置缓存,避免频繁探测与重复定位
    pub map_cache: Mutex<MapCache>,
}

impl AppState {
    /// 创建新的应用状态实例
    #[cfg(test)]
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        Self::with_config_path(config, PathBuf::from("config.yaml"))
    }

    /// 覆盖 Clash API 地址,用于隔离依赖真实控制面的测试
    /// (开发机上 systemd 常驻服务会占用 6262,测试必须指向不可达地址)
    #[cfg(test)]
    pub fn with_clash_api_base(mut self, base: &str) -> Self {
        self.clash_api_base = base.to_string();
        self
    }

    pub fn with_config_path(config: Config, config_path: PathBuf) -> Result<Self, reqwest::Error> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            config: RwLock::new(config),
            route_mode_override: RwLock::new(None),
            config_path,
            clash_api_base: DEFAULT_CLASH_API_BASE.to_string(),
            config_update: Mutex::new(()),
            sing_process: Mutex::new(None),
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
            geo: GeoIp::open_default(),
            map_cache: Mutex::new(MapCache::default()),
        })
    }
}

/// 地图模式位置缓存(本机出口 / 代理节点),TTL 由调用方控制
#[derive(Default)]
pub struct MapCache {
    pub self_location: Option<CachedLocation>,
    pub proxy_location: Option<CachedProxyLocation>,
}

pub struct CachedLocation {
    pub fetched_at: Instant,
    pub ip: String,
    pub point: Option<GeoPoint>,
}

pub struct CachedProxyLocation {
    pub node: String,
    pub fetched_at: Instant,
    pub ip: Option<String>,
    pub point: Option<GeoPoint>,
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
            location: None,
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
            location: None,
        };

        let state = AppState::new(config).unwrap();
        let cache = state.version_cache.load();

        assert!(cache.release.is_none());
        assert!(cache.fetched_at.is_none());
    }
}
