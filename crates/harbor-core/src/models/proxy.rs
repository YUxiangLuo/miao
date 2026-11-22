use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LastProxy {
    pub group: String,
    pub name: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SwitchProxyResult {
    pub changed: bool,
    pub persisted: bool,
}
