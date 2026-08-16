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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfs: Option<Hysteria2Obfs>,
    pub tls: Tls,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Hysteria2Obfs {
    #[serde(rename = "type")]
    pub obfs_type: String,
    pub password: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Tls {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
    pub insecure: bool,
}

#[derive(Deserialize, Default)]
pub struct NodeRequest {
    pub node_type: Option<String>,
    pub tag: String,
    pub server: String,
    pub server_port: u16,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default, alias = "alterId")]
    pub alter_id: Option<u16>,
    #[serde(default)]
    pub sni: Option<String>,
    #[serde(default)]
    pub cipher: Option<String>,
    #[serde(default)]
    pub skip_cert_verify: bool,
    #[serde(default)]
    pub tls_enabled: Option<bool>,
    #[serde(default)]
    pub transport_type: Option<String>,
    #[serde(default)]
    pub transport_path: Option<String>,
    #[serde(default)]
    pub transport_host: Option<String>,
    #[serde(default)]
    pub grpc_service_name: Option<String>,
    #[serde(default)]
    pub alpn: Option<Vec<String>>,
    #[serde(default)]
    pub client_fingerprint: Option<String>,
    #[serde(default)]
    pub reality_public_key: Option<String>,
    #[serde(default)]
    pub reality_short_id: Option<String>,
    #[serde(default)]
    pub flow: Option<String>,
    #[serde(default)]
    pub packet_encoding: Option<String>,
    #[serde(default)]
    pub tuic_congestion_control: Option<String>,
    #[serde(default)]
    pub tuic_udp_relay_mode: Option<String>,
    #[serde(default)]
    pub tuic_zero_rtt: bool,
    #[serde(default)]
    pub obfs_type: Option<String>,
    #[serde(default)]
    pub obfs_password: Option<String>,
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
    use super::{Hysteria2, Hysteria2Obfs, Tls};

    #[test]
    fn hysteria2_serializes_obfs_when_enabled() {
        let node = Hysteria2 {
            outbound_type: "hysteria2".to_string(),
            tag: "hy2-obfs".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            password: "password123".to_string(),
            up_mbps: None,
            down_mbps: None,
            obfs: Some(Hysteria2Obfs {
                obfs_type: "salamander".to_string(),
                password: "obfs-secret".to_string(),
            }),
            tls: Tls {
                enabled: true,
                server_name: None,
                insecure: false,
            },
        };

        let value = serde_json::to_value(node).unwrap();

        assert_eq!(value["obfs"]["type"], "salamander");
        assert_eq!(value["obfs"]["password"], "obfs-secret");
    }

    #[test]
    fn hysteria2_omits_obfs_when_disabled() {
        let node = Hysteria2 {
            outbound_type: "hysteria2".to_string(),
            tag: "hy2-no-obfs".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            password: "password123".to_string(),
            up_mbps: None,
            down_mbps: None,
            obfs: None,
            tls: Tls {
                enabled: true,
                server_name: None,
                insecure: false,
            },
        };

        let value = serde_json::to_value(node).unwrap();

        assert!(value.get("obfs").is_none());
    }
}
