use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiModel {
    /// Public, unique model ID presented to clients.
    pub id: String,
    pub upstream: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiProviderInput {
    pub id: String,
    pub name: String,
    pub base_url: String,
    /// None preserves the existing key. Keys are never returned by GET.
    pub api_key: Option<String>,
    pub responses: bool,
    pub models: Vec<AiModel>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiProviderView {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub has_key: bool,
    pub responses: bool,
    pub models: Vec<AiModel>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiSettingsInput {
    pub revision: u64,
    pub enabled: bool,
    pub port: u16,
    pub proxy_port: u16,
    pub outbound: String,
    pub providers: Vec<AiProviderInput>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiStatus {
    pub enabled: bool,
    pub ready: bool,
    pub port: u16,
    pub outbound: String,
    pub error: Option<String>,
    pub admin_token_path: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiSettingsView {
    pub revision: u64,
    pub enabled: bool,
    pub port: u16,
    pub proxy_port: u16,
    pub outbound: String,
    pub providers: Vec<AiProviderView>,
    pub has_access_key: bool,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiAccessKey {
    pub key: String,
    pub revision: u64,
}
