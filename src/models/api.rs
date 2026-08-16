use serde::{Deserialize, Serialize};

use crate::models::config::RouteMode;

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(message: impl Into<String>, data: T) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: Some(data),
        }
    }

    pub fn success_no_data(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            data: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Serialize)]
pub struct StatusData {
    pub running: bool,
    pub initializing: bool,
    pub route_mode: RouteMode,
    pub adblock: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ConnectivityResult {
    pub name: String,
    pub url: String,
    pub latency_ms: Option<u64>,
    pub success: bool,
}

#[derive(Deserialize)]
pub struct SubRequest {
    pub url: String,
}

#[derive(Deserialize)]
pub struct RuleRequest {
    pub field: String,
    pub value: String,
    pub target: String,
}

#[derive(Deserialize)]
pub struct DeleteRuleRequest {
    pub index: usize,
    pub raw: String,
}

#[derive(Deserialize)]
pub struct AdblockRequest {
    pub enabled: bool,
}

#[derive(Deserialize)]
pub struct VpsDeployRequest {
    pub ip: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct VpsDeployResponse {
    pub tag: String,
}

/// 自定义规则展示项;手写的任意 JSON 规则可能不是结构化单条件,以 raw 兜底
#[derive(Serialize)]
pub struct RuleInfo {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// 出口节点不存在,生成配置时被跳过(未生效)
    pub skipped: bool,
    pub raw: String,
}

#[derive(Deserialize)]
pub struct RouteModeRequest {
    pub route_mode: RouteMode,
}

#[derive(Clone, Serialize)]
pub struct MapSelfPoint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    pub lat: f64,
    pub lng: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct MapProxyPoint {
    pub node: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    pub lat: f64,
    pub lng: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct MapConnection {
    pub ip: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub network: String,
    pub lat: f64,
    pub lng: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    pub up: u64,
    pub down: u64,
    /// 流量经代理出口(chains 中不含 direct)
    pub proxied: bool,
}

#[derive(Serialize)]
pub struct MapOverview {
    /// sing-box 是否在运行(Clash API 可达)
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_point: Option<MapSelfPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_point: Option<MapProxyPoint>,
    pub connections: Vec<MapConnection>,
}

#[derive(Clone, Serialize)]
pub struct SubStatus {
    pub url: String,
    pub success: bool,
    pub node_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
