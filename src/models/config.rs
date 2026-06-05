use serde::{Deserialize, Serialize};

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
        };

        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(!yaml.contains("vps_ip"));
    }
}
