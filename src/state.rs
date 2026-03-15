use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
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

lazy_static! {
    pub static ref SING_PROCESS: Mutex<Option<SingBoxProcess>> = Mutex::new(None);
    pub static ref SUB_STATUS: Mutex<HashMap<String, SubStatus>> = Mutex::new(HashMap::new());
    pub static ref CONFIG_WARNING: Mutex<Option<String>> = Mutex::new(None);
}
