use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteMode {
    #[default]
    Rule,
    Global,
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

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default)]
    pub subs: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub custom_rules: Vec<String>,
    /// 去广告开关:命中内置广告规则集的域名在路由层 reject;
    /// 不拦 DNS,自定义放行规则才能对误拦域名生效
    #[serde(default)]
    pub adblock: bool,
    /// MCP 端点（POST /mcp，JSON-RPC 工具接口）开关。
    /// Linux 面板绑 0.0.0.0，开启后局域网可达——默认关闭，按需显式开启。
    #[serde(default)]
    pub mcp: bool,
    #[serde(default, skip_serializing_if = "NodeSelect::serde_is_manual")]
    pub node_select: NodeSelect,
    #[serde(default, skip_serializing, skip_deserializing)]
    pub route_mode: RouteMode,
}

pub const DEFAULT_PORT: u16 = 6161;

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
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        let out = serde_yaml::to_string(&config).unwrap();
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
            adblock: false,
            mcp: false,
            node_select: Default::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

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
            adblock: false,
            mcp: false,
            node_select: Default::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(!yaml.contains("route_mode"));
    }

    #[test]
    fn config_ignores_route_mode_when_deserializing() {
        let yaml = r#"
port: 6161
route_mode: definitely-not-valid
subs: []
nodes: []
custom_rules: []
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(config.route_mode, super::RouteMode::Rule);
    }

    #[test]
    fn config_omits_default_manual_node_select() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(!yaml.contains("node_select"));
    }

    #[test]
    fn config_persists_fastest_node_select() {
        let config = Config {
            node_select: super::NodeSelect::Fastest(super::Region::Hk),
            ..Config::default()
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("node_select: fastest_hk"));

        let loaded: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            loaded.node_select,
            super::NodeSelect::Fastest(super::Region::Hk)
        );
    }

    #[test]
    fn config_unknown_node_select_falls_back_to_manual() {
        let yaml = r#"
port: 6161
node_select: fastest_kr
subs: []
nodes: []
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.node_select, super::NodeSelect::Manual);
    }
}
