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

/// Metadata only. No request/response bodies, headers, URLs or credentials.
#[derive(Clone, Debug, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiRequestRecord {
    pub id: i64,
    pub timestamp: i64,
    pub provider: String,
    pub model: String,
    pub upstream_model: String,
    pub endpoint: String,
    pub streaming: bool,
    pub outcome: String,
    pub http_status: Option<i32>,
    pub duration_ms: i64,
    pub first_byte_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub usage_complete: bool,
}

#[derive(Debug, Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiUsageGroup {
    pub provider: String,
    pub model: String,
    pub requests: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub cancelled: i64,
    pub usage_known: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub avg_duration_ms: Option<f64>,
    pub p95_duration_ms: Option<f64>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AiAnalytics {
    pub since: i64,
    pub until: i64,
    pub totals: AiUsageGroup,
    pub groups: Vec<AiUsageGroup>,
    pub records: Vec<AiRequestRecord>,
    pub next_before: Option<i64>,
    pub dropped_records: u64,
    pub collection_error: Option<String>,
}
