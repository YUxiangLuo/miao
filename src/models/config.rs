use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteMode {
    #[default]
    Rule,
    Global,
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
}
