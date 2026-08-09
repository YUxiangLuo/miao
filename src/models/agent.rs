use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AgentProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AgentStatusData {
    pub supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub installed: bool,
    pub configured: bool,
    pub session_active: bool,
    pub version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub providers: Vec<AgentProviderInfo>,
    pub required_space_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_space_bytes: Option<u64>,
    pub required_tmp_inodes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_tmp_inodes: Option<u64>,
    pub required_memory_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_memory_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfigRequest {
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    pub api_key: String,
}
