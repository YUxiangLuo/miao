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
