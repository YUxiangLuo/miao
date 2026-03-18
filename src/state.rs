use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::models::{Config, GitHubRelease, SubStatus};

/// 应用状态容器 - 包含所有运行时状态
/// 通过依赖注入传递，避免全局静态变量
pub struct AppState {
    pub config: Mutex<Config>,
    pub sing_process: Mutex<Option<SingBoxProcess>>,
    pub sub_status: Mutex<HashMap<String, SubStatus>>,
    pub config_warning: Mutex<Option<String>>,
    pub initializing: AtomicBool,
    pub http_client: reqwest::Client,
    pub version_cache: Mutex<VersionCache>,
}

impl AppState {
    /// 创建新的应用状态实例
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            config: Mutex::new(config),
            sing_process: Mutex::new(None),
            sub_status: Mutex::new(HashMap::new()),
            config_warning: Mutex::new(None),
            initializing: AtomicBool::new(true),
            http_client,
            version_cache: Mutex::new(VersionCache {
                release: None,
                fetched_at: None,
            }),
        })
    }
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
        };
        
        let state = AppState::new(config.clone()).unwrap();
        
        // 验证状态正确初始化
        assert!(state.initializing.load(std::sync::atomic::Ordering::Relaxed));
        
        // 验证配置被正确存储
        let locked_config = tokio::runtime::Runtime::new().unwrap().block_on(async {
            state.config.lock().await.clone()
        });
        assert_eq!(locked_config.port, Some(8080));
        assert_eq!(locked_config.subs.len(), 1);
    }

    #[test]
    fn version_cache_starts_empty() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
        };
        
        let state = AppState::new(config).unwrap();
        let cache = tokio::runtime::Runtime::new().unwrap().block_on(async {
            state.version_cache.lock().await.clone()
        });
        
        assert!(cache.release.is_none());
        assert!(cache.fetched_at.is_none());
    }
}
