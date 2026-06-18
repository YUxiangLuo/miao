use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteMode {
    #[default]
    Rule,
    Global,
}

impl RouteMode {
    pub fn is_rule(&self) -> bool {
        matches!(self, Self::Rule)
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default)]
    pub subs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vps_ip: Option<String>,
    #[serde(default)]
    pub nodes: Vec<String>,
    #[serde(default)]
    pub custom_rules: Vec<String>,
    #[serde(default, skip_serializing_if = "RouteMode::is_rule")]
    pub route_mode: RouteMode,
}

pub const DEFAULT_PORT: u16 = 6161;

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn config_serializes_vps_ip_when_present() {
        let config = Config {
            port: None,
            subs: vec![],
            vps_ip: Some("203.0.113.10".to_string()),
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(yaml.contains("vps_ip: 203.0.113.10"));
    }

    #[test]
    fn config_omits_empty_vps_ip() {
        let config = Config {
            port: None,
            subs: vec![],
            vps_ip: None,
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(!yaml.contains("vps_ip"));
    }

    #[test]
    fn config_serializes_global_route_mode_when_selected() {
        let config = Config {
            port: None,
            subs: vec![],
            vps_ip: None,
            nodes: vec![],
            custom_rules: vec![],
            route_mode: super::RouteMode::Global,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(yaml.contains("route_mode: global"));
    }

    #[test]
    fn config_omits_default_rule_route_mode() {
        let config = Config {
            port: None,
            subs: vec![],
            vps_ip: None,
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(!yaml.contains("route_mode"));
    }
}
