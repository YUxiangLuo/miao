use serde::{Deserialize, Serialize};

use crate::models::config::{NodeSelect, RouteMode};

/// User-visible runtime lifecycle. `running` remains the process-presence
/// compatibility field; this phase and `ready` describe whether the data plane
/// can actually be used and what work is currently happening.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RuntimePhase {
    #[default]
    Initializing = 0,
    Extracting = 1,
    Validating = 2,
    FetchingSubscriptions = 3,
    Starting = 4,
    Ready = 5,
    RefreshingSubscriptions = 6,
    ApplyingConfig = 7,
    Reloading = 8,
    Stopping = 9,
    Stopped = 10,
    Failed = 11,
}

impl RuntimePhase {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Extracting,
            2 => Self::Validating,
            3 => Self::FetchingSubscriptions,
            4 => Self::Starting,
            5 => Self::Ready,
            6 => Self::RefreshingSubscriptions,
            7 => Self::ApplyingConfig,
            8 => Self::Reloading,
            9 => Self::Stopping,
            10 => Self::Stopped,
            11 => Self::Failed,
            _ => Self::Initializing,
        }
    }
}

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
    /// True only after the managed sing-box instance passed its startup
    /// readiness check. A spawned process may be `running` while this is false.
    pub ready: bool,
    pub phase: RuntimePhase,
    pub initializing: bool,
    pub route_mode: RouteMode,
    pub node_select: NodeSelect,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Structured diagnostics for new clients. `warning` remains above as a
    /// compatibility projection for existing panel/API consumers.
    pub warnings: Vec<RuntimeWarning>,
    pub vps_supported: bool,
    pub platform: &'static str,
    /// MCP 端点（POST /mcp）开关状态
    pub mcp: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RuntimeWarning {
    pub code: &'static str,
    pub message: String,
    pub severity: &'static str,
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

/// clash-verge-rev 导入：单条订阅 + 是否已在 miao 配置中。
#[derive(Serialize)]
pub struct VergeImportItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub url: String,
    pub already_added: bool,
}

/// clash-verge-rev 导入扫描结果；found=false 时 items 为空。
#[derive(Serialize)]
pub struct VergeImportResult {
    pub found: bool,
    pub items: Vec<VergeImportItem>,
}

/// 批量添加订阅：一次配置事务提交全部，跳过已存在的。
#[derive(Deserialize)]
pub struct SubBatchRequest {
    pub urls: Vec<String>,
}

#[derive(Serialize)]
pub struct SubBatchResult {
    pub added: usize,
    pub skipped: usize,
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
pub struct McpRequest {
    pub enabled: bool,
}

#[cfg(not(windows))]
#[derive(Deserialize)]
pub struct VpsDeployRequest {
    pub ip: String,
    pub password: String,
}

#[cfg(not(windows))]
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

#[derive(Deserialize)]
pub struct NodeSelectRequest {
    pub node_select: String,
}

#[derive(Clone, Serialize)]
pub struct SubStatus {
    pub url: String,
    pub success: bool,
    pub node_count: usize,
    pub state: SubscriptionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionState {
    #[default]
    Pending,
    Refreshing,
    Ready,
    Failed,
}
