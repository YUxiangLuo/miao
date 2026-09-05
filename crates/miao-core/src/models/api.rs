use serde::{Deserialize, Serialize};

use crate::models::config::{NodeSelect, RouteMode};

/// User-visible runtime lifecycle. `running` remains the process-presence
/// compatibility field; this phase and `ready` describe whether the data plane
/// can actually be used and what work is currently happening.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct StatusData {
    /// Revision of subscription, node and rule read projections.
    pub data_revision: u64,
    pub running: bool,
    /// True only after the managed sing-box instance passed its startup
    /// readiness check. A spawned process may be `running` while this is false.
    pub ready: bool,
    pub phase: RuntimePhase,
    pub initializing: bool,
    pub route_mode: RouteMode,
    /// 当前运行配置实际生效的选择策略；地区无候选时可能回退 manual。
    pub node_select: NodeSelect,
    /// 用户请求并持久化的选择策略；即使临时回退 manual 也保持 fastest_*。
    pub requested_node_select: NodeSelect,
    /// 当前最高倍率；None 表示不限。字符串保持十进制精度并供 select 直接使用。
    pub max_multiplier: Option<String>,
    /// 当前完整节点池中动态识别出的倍率，按数值升序排列。
    pub multiplier_options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub uptime_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub warning: Option<String>,
    /// Structured diagnostics for new clients. `warning` remains above as a
    /// compatibility projection for existing panel/API consumers.
    pub warnings: Vec<RuntimeWarning>,
    pub vps_supported: bool,
    pub platform: &'static str,
    /// MCP 端点（POST /mcp）开关状态
    pub mcp: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)] // Error/Info are part of the stable API contract for future diagnostics.
pub enum RuntimeWarningSeverity {
    Warning,
    Error,
    Info,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RuntimeWarning {
    pub code: &'static str,
    pub message: String,
    pub severity: RuntimeWarningSeverity,
}

#[derive(Serialize, Clone)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConnectivityResult {
    pub name: String,
    pub url: String,
    pub latency_ms: Option<u64>,
    pub success: bool,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubRequest {
    pub url: String,
}

/// 订阅详情弹窗用的单条订阅节点：展示字段 + 禁用标记。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubNodeInfo {
    pub name: String,
    pub server: String,
    pub server_port: u16,
    pub node_type: String,
    pub disabled: bool,
}

/// 一个订阅及其节点列表（快照里没有节点时 nodes 为空）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubNodesInfo {
    pub url: String,
    pub nodes: Vec<SubNodeInfo>,
    /// 失配的禁用条目名（订阅刷新后节点改名/消失，条目不再生效），供面板展示与清理
    pub stale_disabled: Vec<String>,
}

/// 禁用/启用订阅节点：按订阅 URL + 节点名标识（同名节点连坐）。
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetNodeDisabledRequest {
    pub sub: String,
    pub name: String,
    pub disabled: bool,
}

/// clash-verge-rev 导入：单条订阅 + 是否已在 miao 配置中。
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VergeImportItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub name: Option<String>,
    pub url: String,
    pub already_added: bool,
}

/// clash-verge-rev 导入扫描结果；found=false 时 items 为空。
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VergeImportResult {
    pub found: bool,
    pub items: Vec<VergeImportItem>,
}

/// 批量添加订阅：一次配置事务提交全部，跳过已存在的。
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubBatchRequest {
    pub urls: Vec<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubBatchResult {
    pub added: usize,
    pub skipped: usize,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RuleRequest {
    pub field: String,
    pub value: String,
    pub target: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeleteRuleRequest {
    pub index: usize,
    pub raw: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpRequest {
    pub enabled: bool,
}

#[cfg(not(windows))]
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VpsDeployRequest {
    pub ip: String,
    pub password: String,
}

#[cfg(not(windows))]
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VpsDeployResponse {
    pub tag: String,
}

/// 自定义规则展示项;手写的任意 JSON 规则可能不是结构化单条件,以 raw 兜底
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RuleInfo {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
    /// 出口节点不存在,生成配置时被跳过(未生效)
    pub skipped: bool,
    pub raw: String,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RouteModeRequest {
    pub route_mode: RouteMode,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeSelectRequest {
    #[cfg_attr(test, ts(type = "NodeSelect"))]
    pub node_select: String,
}

#[cfg_attr(test, derive(ts_rs::TS))]
pub struct MaxMultiplierRequest {
    /// null = 不限；其他值为不带 x 的正十进制字符串。
    pub max_multiplier: Option<String>,
}

impl<'de> Deserialize<'de> for MaxMultiplierRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct RequiredField {
            // Value 不是 Option，因此字段缺失会由 serde 返回 missing field，而显式
            // null 仍可与字符串区分。
            max_multiplier: serde_json::Value,
        }

        let raw = RequiredField::deserialize(deserializer)?.max_multiplier;
        let max_multiplier = match raw {
            serde_json::Value::Null => None,
            serde_json::Value::String(value) => Some(value),
            _ => {
                return Err(serde::de::Error::custom(
                    "max_multiplier must be a string or null",
                ));
            }
        };
        Ok(Self { max_multiplier })
    }
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubStatus {
    pub url: String,
    pub success: bool,
    pub node_count: usize,
    /// 该订阅被禁用的节点数（易变层 disabled_nodes 中匹配此订阅的条目数）
    #[serde(default)]
    pub disabled_count: usize,
    pub state: SubscriptionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionState {
    #[default]
    Pending,
    Refreshing,
    Ready,
    Failed,
}
