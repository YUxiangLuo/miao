use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Hysteria2 {
    #[serde(rename = "type")]
    pub outbound_type: String,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_mbps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub down_mbps: Option<u32>,
    pub tls: Tls,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tls {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    pub insecure: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AnyTls {
    #[serde(rename = "type")]
    pub outbound_type: String,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub password: String,
    pub tls: Tls,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Shadowsocks {
    #[serde(rename = "type")]
    pub outbound_type: String,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub method: String,
    pub password: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Trojan {
    #[serde(rename = "type")]
    pub outbound_type: String,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub password: String,
    pub tls: Tls,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VMess {
    #[serde(rename = "type")]
    pub outbound_type: String,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alter_id: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<Tls>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Vless {
    #[serde(rename = "type")]
    pub outbound_type: String,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
    pub tls: Tls,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tuic {
    #[serde(rename = "type")]
    pub outbound_type: String,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub uuid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub congestion_control: Option<String>,
    pub tls: Tls,
}

#[derive(Deserialize)]
pub struct NodeRequest {
    pub node_type: Option<String>,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub password: String,
    #[serde(default)]
    pub sni: Option<String>,
    #[serde(default)]
    pub cipher: Option<String>,
    #[serde(default)]
    pub skip_cert_verify: bool,
}

#[derive(Deserialize)]
pub struct DeleteNodeRequest {
    pub tag: String,
}

#[derive(Serialize)]
pub struct NodeInfo {
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sni: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trojan_serializes_to_expected_json() {
        let node = Trojan {
            outbound_type: "trojan".to_string(),
            tag: "test-tr".to_string(),
            server: "tr.example.com".to_string(),
            server_port: 443,
            password: "secret".to_string(),
            tls: Tls {
                enabled: true,
                server_name: Some("sni.example.com".to_string()),
                insecure: false,
            },
        };
        let json = serde_json::to_string(&node).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "trojan");
        assert_eq!(parsed["tag"], "test-tr");
        assert_eq!(parsed["password"], "secret");
        assert_eq!(parsed["tls"]["server_name"], "sni.example.com");
    }

    #[test]
    fn vmess_serializes_security_and_alter_id() {
        let node = VMess {
            outbound_type: "vmess".to_string(),
            tag: "test-vm".to_string(),
            server: "vm.example.com".to_string(),
            server_port: 443,
            uuid: "bf000d23-0752-40b4-affe-68f7707a9661".to_string(),
            security: Some("auto".to_string()),
            alter_id: Some(0),
            tls: Some(Tls {
                enabled: true,
                server_name: None,
                insecure: false,
            }),
        };
        let json = serde_json::to_string(&node).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "vmess");
        assert_eq!(parsed["uuid"], "bf000d23-0752-40b4-affe-68f7707a9661");
        assert_eq!(parsed["security"], "auto");
        assert_eq!(parsed["alter_id"], 0);
    }

    #[test]
    fn vless_serializes_flow_and_tls() {
        let node = Vless {
            outbound_type: "vless".to_string(),
            tag: "test-vl".to_string(),
            server: "vl.example.com".to_string(),
            server_port: 443,
            uuid: "bf000d23-0752-40b4-affe-68f7707a9661".to_string(),
            flow: Some("xtls-rprx-vision".to_string()),
            tls: Tls {
                enabled: true,
                server_name: Some("vl.example.com".to_string()),
                insecure: false,
            },
        };
        let json = serde_json::to_string(&node).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "vless");
        assert_eq!(parsed["uuid"], "bf000d23-0752-40b4-affe-68f7707a9661");
        assert_eq!(parsed["flow"], "xtls-rprx-vision");
        assert_eq!(parsed["tls"]["enabled"], true);
    }

    #[test]
    fn tuic_serializes_congestion_control() {
        let node = Tuic {
            outbound_type: "tuic".to_string(),
            tag: "test-tu".to_string(),
            server: "tu.example.com".to_string(),
            server_port: 443,
            uuid: "bf000d23-0752-40b4-affe-68f7707a9661".to_string(),
            token: None,
            congestion_control: Some("bbr".to_string()),
            tls: Tls {
                enabled: true,
                server_name: None,
                insecure: true,
            },
        };
        let json = serde_json::to_string(&node).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["type"], "tuic");
        assert_eq!(parsed["uuid"], "bf000d23-0752-40b4-affe-68f7707a9661");
        assert_eq!(parsed["congestion_control"], "bbr");
        assert_eq!(parsed["tls"]["insecure"], true);
    }

    #[test]
    fn tls_skips_none_server_name_when_serializing() {
        let node = Trojan {
            outbound_type: "trojan".to_string(),
            tag: "no-sni".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            password: "p".to_string(),
            tls: Tls {
                enabled: true,
                server_name: None,
                insecure: false,
            },
        };
        let json = serde_json::to_string(&node).unwrap();
        assert!(!json.contains("server_name"));
    }
}
