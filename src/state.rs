use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

use crate::models::{Config, SubStatus};

/// 应用状态容器 - 包含所有运行时状态
/// 通过依赖注入传递，避免全局静态变量
pub struct AppState {
    pub config: Mutex<Config>,
    pub sing_process: Mutex<Option<SingBoxProcess>>,
    pub sub_status: Mutex<HashMap<String, SubStatus>>,
    pub config_warning: Mutex<Option<String>>,
    pub initializing: AtomicBool,
    pub http_client: reqwest::Client,
}

impl AppState {
    /// 创建新的应用状态实例
    pub fn new(config: Config) -> Self {
        Self {
            config: Mutex::new(config),
            sing_process: Mutex::new(None),
            sub_status: Mutex::new(HashMap::new()),
            config_warning: Mutex::new(None),
            initializing: AtomicBool::new(true),
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

}

pub struct SingBoxProcess {
    pub child: tokio::process::Child,
    pub started_at: Instant,
}

/// 版本信息缓存
pub struct VersionCache {
    pub release: Option<crate::models::GitHubRelease>,
    pub fetched_at: Option<Instant>,
}

/// 全局版本缓存 - 使用 RwLock 支持并发读取
pub static VERSION_CACHE: RwLock<VersionCache> = RwLock::const_new(VersionCache {
    release: None,
    fetched_at: None,
});
