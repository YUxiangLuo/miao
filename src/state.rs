use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::LazyLock;
use std::time::Instant;
use tokio::sync::Mutex;

use crate::models::{Config, SubStatus};

pub struct AppState {
    pub config: Mutex<Config>,
}

pub struct SingBoxProcess {
    pub child: tokio::process::Child,
    pub started_at: Instant,
}

pub static INITIALIZING: AtomicBool = AtomicBool::new(true);

pub static SING_PROCESS: LazyLock<Mutex<Option<SingBoxProcess>>> =
    LazyLock::new(|| Mutex::new(None));
pub static SUB_STATUS: LazyLock<Mutex<HashMap<String, SubStatus>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub static CONFIG_WARNING: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

pub static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
});
