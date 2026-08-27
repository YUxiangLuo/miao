//! Panel-facing config/runtime warning copy. Keep these strings in one place
//! so REST, MCP and startup recovery cannot drift.

pub const REGION_FALLBACK: &str = "该地区没有可用节点，已切回手动选择";
pub const ALL_SUBS_FAILED: &str = "所有订阅获取失败，请检查当前订阅";
pub const ALL_SUBS_FAILED_KEEP_CACHE: &str =
    "所有订阅获取失败，继续使用当前配置运行，网络恢复后将自动重试";
pub const ALL_SUBS_FAILED_RETRY: &str = "所有订阅获取失败，网络恢复后将自动重试";
pub const REFRESH_VALIDATION_FAILED: &str =
    "订阅刷新后的配置校验失败，继续使用当前配置运行，稍后将自动重试";
pub const REFRESH_FAILED_KEEP_CACHE: &str =
    "订阅刷新失败，继续使用当前配置运行，网络恢复后将自动重试";
pub const STARTUP_VALIDATION_RETRY: &str = "订阅配置校验失败，修复订阅后将自动重试";
pub const DATA_PLANE_RETRYING: &str = "代理服务仍未就绪，正在后台自动重试";
pub const SUBS_REFRESHING_MANUAL: &str = "订阅正在后台刷新，暂时使用手动节点";
pub const NO_USABLE_MANUAL: &str = "没有可用的手动节点，请检查配置或添加节点";
pub const NO_USABLE_SUBS: &str = "所有订阅获取失败且没有可用手动节点，请检查订阅或添加节点";
