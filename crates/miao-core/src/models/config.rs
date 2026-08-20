use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteMode {
    #[default]
    Rule,
    Global,
}

impl RouteMode {
    fn serde_is_rule(mode: &Self) -> bool {
        matches!(mode, Self::Rule)
    }
}

impl<'de> Deserialize<'de> for RouteMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        // 配置文件容错：未知值当规则分流，避免手写 yaml 导致启动失败
        Ok(match raw.trim().to_ascii_lowercase().as_str() {
            "global" => Self::Global,
            _ => Self::Rule,
        })
    }
}

/// 节点池选择策略，写入 config.yaml。
/// `manual`：selector 全量节点；`fastest_*`：按地区筛进 urltest。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeSelect {
    #[default]
    Manual,
    Fastest(Region),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Hk,
    Jp,
    Tw,
    Sg,
    Us,
}

impl NodeSelect {
    pub const fn is_manual(self) -> bool {
        matches!(self, Self::Manual)
    }

    fn serde_is_manual(select: &Self) -> bool {
        select.is_manual()
    }

    pub const fn region(self) -> Option<Region> {
        match self {
            Self::Manual => None,
            Self::Fastest(region) => Some(region),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Fastest(Region::Hk) => "fastest_hk",
            Self::Fastest(Region::Jp) => "fastest_jp",
            Self::Fastest(Region::Tw) => "fastest_tw",
            Self::Fastest(Region::Sg) => "fastest_sg",
            Self::Fastest(Region::Us) => "fastest_us",
        }
    }

    /// 面板/API 入参：只接受已知取值。
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "manual" => Some(Self::Manual),
            "fastest_hk" => Some(Self::Fastest(Region::Hk)),
            "fastest_jp" => Some(Self::Fastest(Region::Jp)),
            "fastest_tw" => Some(Self::Fastest(Region::Tw)),
            "fastest_sg" => Some(Self::Fastest(Region::Sg)),
            "fastest_us" => Some(Self::Fastest(Region::Us)),
            _ => None,
        }
    }
}

impl Serialize for NodeSelect {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NodeSelect {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        // 配置文件容错：未知值当手动，避免旧/手写 yaml 导致启动失败
        Ok(Self::parse(&raw).unwrap_or(Self::Manual))
    }
}

/// Runtime-effective configuration.  This is deliberately separate from
/// [`StableConfig`]: `node_select` and `route_mode` may have been overlaid by
/// volatile preferences and must never overwrite their boot defaults in
/// `config.yaml` as a side effect of an unrelated save.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default)]
    pub subs: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub custom_rules: Vec<String>,
    /// MCP 端点（POST /mcp，JSON-RPC 工具接口）开关。
    /// Linux 面板绑 0.0.0.0，开启后局域网可达——默认关闭，按需显式开启。
    #[serde(default)]
    pub mcp: bool,
    /// 节点选择策略：易变层字段——仍从 config.yaml 反序列化（向后兼容旧配置），
    /// 但不再写入稳定层；运行值落 volatile.yaml。
    #[serde(default, skip_serializing)]
    pub node_select: NodeSelect,
    /// 路由模式：易变层字段——config.yaml 里的值是启动默认值（volatile 缺失时生效），
    /// 运行值落 volatile.yaml，稳定层保存不再携带。
    #[serde(default, skip_serializing)]
    pub route_mode: RouteMode,
}

/// The exact semantic model persisted in `config.yaml`.
///
/// The YAML shape remains identical to older versions.  In particular the
/// optional `node_select` and `route_mode` keys continue to mean boot
/// defaults, used when the volatile layer is absent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default)]
    pub subs: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub custom_rules: Vec<String>,
    #[serde(default)]
    pub mcp: bool,
    #[serde(default, skip_serializing_if = "NodeSelect::serde_is_manual")]
    pub node_select: NodeSelect,
    #[serde(default, skip_serializing_if = "RouteMode::serde_is_rule")]
    pub route_mode: RouteMode,
}

pub const DEFAULT_PORT: u16 = 6161;

/// 易变配置层：高频变更的运行选择（节点选择策略 / 路由模式）。
/// 与稳定层 config.yaml 分文件落盘：OpenWrt/Linux 写 tmpfs（系统重启即回默认），
/// Windows 写应用数据目录（持久）。文件缺失/读不出/损坏等价于「无覆盖」，
/// 此时 config.yaml 里的同名字段（旧版遗留或手写的启动默认值）生效。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolatileConfig {
    #[serde(default, skip_serializing_if = "NodeSelect::serde_is_manual")]
    pub node_select: NodeSelect,
    #[serde(default, skip_serializing_if = "RouteMode::serde_is_rule")]
    pub route_mode: RouteMode,
}

impl From<&Config> for VolatileConfig {
    fn from(config: &Config) -> Self {
        Self {
            node_select: config.node_select,
            route_mode: config.route_mode,
        }
    }
}

impl From<&Config> for StableConfig {
    fn from(config: &Config) -> Self {
        Self {
            port: config.port,
            subs: config.subs.clone(),
            nodes: config.nodes.clone(),
            custom_rules: config.custom_rules.clone(),
            mcp: config.mcp,
            node_select: config.node_select,
            route_mode: config.route_mode,
        }
    }
}

impl StableConfig {
    /// Merge the optional volatile preferences into a runtime view.
    pub fn effective(&self, volatile: Option<VolatileConfig>) -> Config {
        let mut config = Config {
            port: self.port,
            subs: self.subs.clone(),
            nodes: self.nodes.clone(),
            custom_rules: self.custom_rules.clone(),
            mcp: self.mcp,
            node_select: self.node_select,
            route_mode: self.route_mode,
        };
        if let Some(volatile) = volatile {
            config.node_select = volatile.node_select;
            config.route_mode = volatile.route_mode;
        }
        config
    }

    /// Apply low-frequency fields from the effective configuration while
    /// retaining the boot defaults that came from `config.yaml`.
    pub fn with_stable_fields_from(&self, config: &Config) -> Self {
        Self {
            port: config.port,
            subs: config.subs.clone(),
            nodes: config.nodes.clone(),
            custom_rules: config.custom_rules.clone(),
            mcp: config.mcp,
            node_select: self.node_select,
            route_mode: self.route_mode,
        }
    }
}

#[cfg(test)]
impl Config {
    /// 用易变层覆盖出运行时合并视图：volatile > config.yaml > 默认。
    /// `None`（易变文件缺失/损坏）时保留 config.yaml 的解析结果。
    pub fn overlay(mut self, volatile: Option<VolatileConfig>) -> Self {
        if let Some(volatile) = volatile {
            self.node_select = volatile.node_select;
            self.route_mode = volatile.route_mode;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn config_ignores_legacy_vps_ip_field() {
        // vps_ip 已废弃:旧配置仍能解析,再次保存时会被丢弃
        let yaml = r#"
port: 6161
vps_ip: 203.0.113.10
subs: []
nodes: []
"#;
        let config: Config = yaml_serde::from_str(yaml).unwrap();
        let out = yaml_serde::to_string(&config).unwrap();
        assert!(!out.contains("vps_ip"));
    }

    #[test]
    fn config_omits_global_route_mode() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: super::RouteMode::Global,
            mcp: false,
            node_select: Default::default(),
        };

        let yaml = yaml_serde::to_string(&config).unwrap();

        assert!(!yaml.contains("route_mode"));
    }

    #[test]
    fn config_omits_default_rule_route_mode() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
            mcp: false,
            node_select: Default::default(),
        };

        let yaml = yaml_serde::to_string(&config).unwrap();

        assert!(!yaml.contains("route_mode"));
    }

    #[test]
    fn config_reads_route_mode_as_boot_default() {
        // config.yaml 的 route_mode 是启动默认值（volatile 缺失时生效）
        let yaml = r#"
port: 6161
route_mode: global
subs: []
nodes: []
custom_rules: []
"#;
        let config: Config = yaml_serde::from_str(yaml).unwrap();
        assert_eq!(config.route_mode, super::RouteMode::Global);

        // 未知值回落规则分流，不让手写 yaml 启动失败
        let config: Config = yaml_serde::from_str("route_mode: definitely-not-valid").unwrap();
        assert_eq!(config.route_mode, super::RouteMode::Rule);
    }

    #[test]
    fn config_omits_default_manual_node_select() {
        let config = Config::default();
        let yaml = yaml_serde::to_string(&config).unwrap();
        assert!(!yaml.contains("node_select"));
    }

    #[test]
    fn config_reads_legacy_node_select_without_persisting() {
        // 旧配置里的 node_select 仍能读出（作为 volatile 缺失时的回落）
        let yaml = r#"
port: 6161
node_select: fastest_hk
subs: []
nodes: []
"#;
        let loaded: Config = yaml_serde::from_str(yaml).unwrap();
        assert_eq!(
            loaded.node_select,
            super::NodeSelect::Fastest(super::Region::Hk)
        );

        // 但稳定层保存不再携带 node_select（已迁入易变层）
        let out = yaml_serde::to_string(&loaded).unwrap();
        assert!(!out.contains("node_select"));
    }

    #[test]
    fn config_unknown_node_select_falls_back_to_manual() {
        let yaml = r#"
port: 6161
node_select: fastest_kr
subs: []
nodes: []
"#;
        let config: Config = yaml_serde::from_str(yaml).unwrap();
        assert_eq!(config.node_select, super::NodeSelect::Manual);
    }

    #[test]
    fn volatile_config_roundtrip() {
        use super::{NodeSelect, Region, RouteMode, VolatileConfig};

        let volatile = VolatileConfig {
            node_select: NodeSelect::Fastest(Region::Jp),
            route_mode: RouteMode::Global,
        };
        let yaml = yaml_serde::to_string(&volatile).unwrap();
        assert!(yaml.contains("node_select: fastest_jp"));
        assert!(yaml.contains("route_mode: global"));

        let loaded: VolatileConfig = yaml_serde::from_str(&yaml).unwrap();
        assert_eq!(loaded, volatile);
    }

    #[test]
    fn volatile_config_omits_defaults() {
        use super::VolatileConfig;

        let yaml = yaml_serde::to_string(&VolatileConfig::default()).unwrap();
        assert!(!yaml.contains("node_select"));
        assert!(!yaml.contains("route_mode"));

        // 全默认（含空文件 `{}`）也能正常解析
        let loaded: VolatileConfig = yaml_serde::from_str("{}").unwrap();
        assert_eq!(loaded, VolatileConfig::default());
    }

    #[test]
    fn overlay_applies_volatile_over_yaml() {
        use super::{Config, NodeSelect, Region, RouteMode, VolatileConfig};

        let yaml_config = Config {
            node_select: NodeSelect::Fastest(Region::Hk),
            route_mode: RouteMode::Global,
            ..Config::default()
        };

        // volatile 显式值覆盖 yaml
        let merged = yaml_config.clone().overlay(Some(VolatileConfig {
            node_select: NodeSelect::Manual,
            route_mode: RouteMode::Rule,
        }));
        assert_eq!(merged.node_select, NodeSelect::Manual);
        assert_eq!(merged.route_mode, RouteMode::Rule);

        // volatile 全默认也是显式覆盖（用户切回了 manual/rule）
        let merged = yaml_config.clone().overlay(Some(VolatileConfig::default()));
        assert_eq!(merged.node_select, NodeSelect::Manual);
        assert_eq!(merged.route_mode, RouteMode::Rule);

        // volatile 文件缺失（None）时保留 yaml 值（旧版配置兼容）
        let merged = yaml_config.overlay(None);
        assert_eq!(merged.node_select, NodeSelect::Fastest(Region::Hk));
        assert_eq!(merged.route_mode, RouteMode::Global);
    }

    #[test]
    fn stable_config_preserves_boot_defaults_when_effective_preferences_change() {
        use super::{NodeSelect, Region, RouteMode, StableConfig, VolatileConfig};

        let stable: StableConfig = yaml_serde::from_str(
            "route_mode: global\nnode_select: fastest_hk\nsubs: []\nnodes: []\n",
        )
        .unwrap();
        let effective = stable.effective(Some(VolatileConfig {
            node_select: NodeSelect::Manual,
            route_mode: RouteMode::Rule,
        }));
        let mut changed = effective;
        changed.mcp = true;
        let saved = stable.with_stable_fields_from(&changed);
        let yaml = yaml_serde::to_string(&saved).unwrap();

        assert!(yaml.contains("route_mode: global"));
        assert!(yaml.contains("node_select: fastest_hk"));
        assert!(yaml.contains("mcp: true"));
        assert_eq!(saved.node_select, NodeSelect::Fastest(Region::Hk));
    }
}
