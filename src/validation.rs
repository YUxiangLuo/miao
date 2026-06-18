use regex::Regex;
use std::sync::LazyLock;

static VALID_TAG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}\-_\s]{1,64}$").unwrap());
static UUID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});

static VALID_NODE_TYPES: &[&str] = &[
    "hysteria2",
    "anytls",
    "ss",
    "vmess",
    "vless",
    "trojan",
    "tuic",
];

static VALID_SS_CIPHERS: &[&str] = &[
    "2022-blake3-aes-128-gcm",
    "2022-blake3-aes-256-gcm",
    "2022-blake3-chacha20-poly1305",
    "aes-128-gcm",
    "aes-256-gcm",
    "chacha20-ietf-poly1305",
];

static VALID_VMESS_CIPHERS: &[&str] = &["auto", "none", "zero", "aes-128-gcm", "chacha20-poly1305"];
static VALID_TRANSPORT_TYPES: &[&str] = &["tcp", "ws", "http", "h2", "grpc"];
static VALID_CLIENT_FINGERPRINTS: &[&str] = &[
    "chrome",
    "firefox",
    "edge",
    "safari",
    "360",
    "qq",
    "ios",
    "android",
    "random",
    "randomized",
];
static VALID_PACKET_ENCODINGS: &[&str] = &["packetaddr", "xudp"];
static VALID_VLESS_FLOWS: &[&str] = &["xtls-rprx-vision"];
static VALID_TUIC_CONGESTION_CONTROLS: &[&str] = &["cubic", "new_reno", "bbr"];
static VALID_TUIC_UDP_RELAY_MODES: &[&str] = &["native", "quic"];
static VALID_HYSTERIA2_OBFS_TYPES: &[&str] = &["salamander", "gecko"];

use crate::models::NodeRequest;

pub struct Validator;

fn non_empty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

impl Validator {
    pub fn validate_node_request(req: &NodeRequest) -> Result<(), String> {
        Self::node_tag(&req.tag)?;
        Self::server_address(&req.server)?;
        Self::port(req.server_port)?;
        let node_type = req.node_type.as_deref().unwrap_or("hysteria2");
        Self::node_type(node_type)?;

        if matches!(node_type, "hysteria2" | "anytls" | "ss" | "trojan" | "tuic") {
            Self::password(&req.password)?;
        }
        if matches!(node_type, "vmess" | "vless" | "tuic") {
            let uuid = non_empty(&req.uuid).ok_or("UUID 不能为空")?;
            Self::uuid(uuid)?;
        }
        if let Some(ref sni) = req.sni {
            Self::sni(sni)?;
        }
        if let Some(ref cipher) = req.cipher {
            if !cipher.trim().is_empty() {
                match node_type {
                    "vmess" => Self::vmess_cipher(cipher)?,
                    "ss" => Self::cipher(cipher)?,
                    _ => return Err(format!("{} 节点不支持加密方式字段", node_type)),
                }
            }
        }
        if let Some(transport_type) = non_empty(&req.transport_type) {
            Self::transport_type(transport_type)?;
            if !matches!(node_type, "vmess" | "vless" | "trojan") {
                return Err(format!("{} 节点不支持传输层配置", node_type));
            }
        }
        if let Some(path) = non_empty(&req.transport_path) {
            Self::transport_path(path)?;
        }
        if let Some(host) = non_empty(&req.transport_host) {
            Self::header_host(host)?;
        }
        if let Some(service_name) = non_empty(&req.grpc_service_name) {
            Self::grpc_service_name(service_name)?;
        }
        if let Some(ref alpn) = req.alpn {
            Self::alpn(alpn)?;
        }
        if let Some(fingerprint) = non_empty(&req.client_fingerprint) {
            Self::client_fingerprint(fingerprint)?;
        }
        if non_empty(&req.reality_public_key).is_some()
            || non_empty(&req.reality_short_id).is_some()
        {
            if node_type != "vless" {
                return Err("只有 VLESS 节点支持 Reality 配置".to_string());
            }
            if non_empty(&req.reality_public_key).is_none() {
                return Err("Reality public key 不能为空".to_string());
            }
            let fingerprint = non_empty(&req.client_fingerprint)
                .ok_or("Reality 节点必须配置 TLS 指纹（uTLS）")?;
            Self::client_fingerprint(fingerprint)?;
        }
        if let Some(flow) = non_empty(&req.flow) {
            if node_type != "vless" {
                return Err("只有 VLESS 节点支持 flow 字段".to_string());
            }
            Self::vless_flow(flow)?;
        }
        if let Some(packet_encoding) = non_empty(&req.packet_encoding) {
            if !matches!(node_type, "vmess" | "vless") {
                return Err("只有 VMess/VLESS 节点支持 packet encoding".to_string());
            }
            Self::packet_encoding(packet_encoding)?;
        }
        if let Some(congestion_control) = non_empty(&req.tuic_congestion_control) {
            if node_type != "tuic" {
                return Err("只有 TUIC 节点支持拥塞控制配置".to_string());
            }
            Self::tuic_congestion_control(congestion_control)?;
        }
        if let Some(udp_relay_mode) = non_empty(&req.tuic_udp_relay_mode) {
            if node_type != "tuic" {
                return Err("只有 TUIC 节点支持 UDP relay mode".to_string());
            }
            Self::tuic_udp_relay_mode(udp_relay_mode)?;
        }
        let has_obfs_password = req
            .obfs_password
            .as_deref()
            .is_some_and(|password| !password.trim().is_empty());
        if node_type != "hysteria2" && (req.obfs_type.is_some() || has_obfs_password) {
            return Err("只有 Hysteria2 节点支持混淆配置".to_string());
        }
        if let Some(ref obfs_type) = req.obfs_type {
            Self::hysteria2_obfs_type(obfs_type)?;
            let password = req.obfs_password.as_deref().unwrap_or_default().trim();
            if password.is_empty() {
                return Err("混淆密码不能为空".to_string());
            }
            if password.len() > 256 {
                return Err("混淆密码过长（最多 256 个字符）".to_string());
            }
        } else if has_obfs_password {
            return Err("请先选择混淆类型".to_string());
        }
        Ok(())
    }

    pub fn subscription_url(url: &str) -> Result<(), String> {
        if url.is_empty() {
            return Err("订阅链接不能为空".to_string());
        }

        if url.len() > 4096 {
            return Err("订阅链接过长".to_string());
        }

        match url::Url::parse(url) {
            Ok(parsed) => {
                let scheme = parsed.scheme();
                if scheme != "http" && scheme != "https" {
                    return Err("订阅链接必须使用 HTTP 或 HTTPS 协议".to_string());
                }

                if parsed.host_str().is_none() {
                    return Err("订阅链接缺少有效的主机名".to_string());
                }

                Ok(())
            }
            Err(_) => Err("无效的订阅链接格式".to_string()),
        }
    }

    pub fn node_tag(tag: &str) -> Result<(), String> {
        if tag.is_empty() {
            return Err("节点名称不能为空".to_string());
        }

        if tag.chars().count() > 64 {
            return Err("节点名称不能超过 64 个字符".to_string());
        }

        if !VALID_TAG_REGEX.is_match(tag) {
            return Err("节点名称只能包含字母、数字、空格、下划线和连字符".to_string());
        }

        Ok(())
    }

    pub fn server_address(server: &str) -> Result<(), String> {
        if server.is_empty() {
            return Err("服务器地址不能为空".to_string());
        }

        if server.len() > 253 {
            return Err("服务器地址过长".to_string());
        }

        // 检查是否为有效的 IPv4 或 IPv6 地址
        if server.parse::<std::net::IpAddr>().is_ok() {
            return Ok(());
        }

        // 处理完全合格的域名（FQDN）末尾的点号
        let server = server.trim_end_matches('.');

        if !server.contains('.') {
            return Err("域名必须包含点号".to_string());
        }

        let parts: Vec<&str> = server.split('.').collect();
        for part in parts {
            if part.is_empty() {
                return Err("域名部分不能为空".to_string());
            }
            if part.len() > 63 {
                return Err("域名的每个部分不能超过 63 个字符".to_string());
            }
            if part.starts_with('-') || part.ends_with('-') {
                return Err("域名部分不能以连字符开头或结尾".to_string());
            }
            if !part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return Err("域名部分只能包含字母、数字和连字符".to_string());
            }
        }

        Ok(())
    }

    pub fn port(port: u16) -> Result<(), String> {
        if port == 0 {
            return Err("端口号不能为 0".to_string());
        }

        Ok(())
    }

    pub fn node_type(node_type: &str) -> Result<(), String> {
        if !VALID_NODE_TYPES.contains(&node_type) {
            return Err(format!(
                "不支持的节点类型: {}，支持的类型: {}",
                node_type,
                VALID_NODE_TYPES.join(", ")
            ));
        }

        Ok(())
    }

    pub fn uuid(uuid: &str) -> Result<(), String> {
        if !UUID_REGEX.is_match(uuid) {
            return Err("UUID 格式无效".to_string());
        }

        Ok(())
    }

    pub fn password(password: &str) -> Result<(), String> {
        if password.is_empty() {
            return Err("密码不能为空".to_string());
        }

        if password.len() < 8 {
            return Err("密码太短（至少 8 个字符）".to_string());
        }

        if password.len() > 256 {
            return Err("密码过长（最多 256 个字符）".to_string());
        }

        Ok(())
    }

    pub fn cipher(cipher: &str) -> Result<(), String> {
        if !VALID_SS_CIPHERS.contains(&cipher) {
            return Err(format!("不支持的加密方式: {}", cipher));
        }

        Ok(())
    }

    pub fn vmess_cipher(cipher: &str) -> Result<(), String> {
        if !VALID_VMESS_CIPHERS.contains(&cipher) {
            return Err(format!("不支持的 VMess 加密方式: {}", cipher));
        }

        Ok(())
    }

    pub fn transport_type(transport_type: &str) -> Result<(), String> {
        if !VALID_TRANSPORT_TYPES.contains(&transport_type) {
            return Err(format!("不支持的传输层类型: {}", transport_type));
        }

        Ok(())
    }

    pub fn transport_path(path: &str) -> Result<(), String> {
        if path.len() > 512 {
            return Err("传输层路径过长".to_string());
        }

        if !path.starts_with('/') {
            return Err("传输层路径必须以 / 开头".to_string());
        }

        Ok(())
    }

    pub fn header_host(host: &str) -> Result<(), String> {
        if host.len() > 253 {
            return Err("Host 过长".to_string());
        }

        if host.chars().any(char::is_whitespace) {
            return Err("Host 不能包含空白字符".to_string());
        }

        Ok(())
    }

    pub fn grpc_service_name(service_name: &str) -> Result<(), String> {
        if service_name.len() > 256 {
            return Err("gRPC service name 过长".to_string());
        }

        Ok(())
    }

    pub fn alpn(alpn: &[String]) -> Result<(), String> {
        for item in alpn {
            let value = item.trim();
            if value.is_empty() {
                return Err("ALPN 不能为空".to_string());
            }
            if value.len() > 32 {
                return Err("ALPN 过长".to_string());
            }
        }

        Ok(())
    }

    pub fn client_fingerprint(fingerprint: &str) -> Result<(), String> {
        let normalized = fingerprint.to_ascii_lowercase();
        if !VALID_CLIENT_FINGERPRINTS.contains(&normalized.as_str()) {
            return Err(format!("不支持的 TLS 指纹: {}", fingerprint));
        }

        Ok(())
    }

    pub fn packet_encoding(packet_encoding: &str) -> Result<(), String> {
        if !VALID_PACKET_ENCODINGS.contains(&packet_encoding) {
            return Err(format!("不支持的 packet encoding: {}", packet_encoding));
        }

        Ok(())
    }

    pub fn vless_flow(flow: &str) -> Result<(), String> {
        if !VALID_VLESS_FLOWS.contains(&flow) {
            return Err(format!("不支持的 VLESS flow: {}", flow));
        }

        Ok(())
    }

    pub fn tuic_congestion_control(congestion_control: &str) -> Result<(), String> {
        if !VALID_TUIC_CONGESTION_CONTROLS.contains(&congestion_control) {
            return Err(format!("不支持的 TUIC 拥塞控制: {}", congestion_control));
        }

        Ok(())
    }

    pub fn tuic_udp_relay_mode(udp_relay_mode: &str) -> Result<(), String> {
        if !VALID_TUIC_UDP_RELAY_MODES.contains(&udp_relay_mode) {
            return Err(format!("不支持的 TUIC UDP relay mode: {}", udp_relay_mode));
        }

        Ok(())
    }

    pub fn hysteria2_obfs_type(obfs_type: &str) -> Result<(), String> {
        if !VALID_HYSTERIA2_OBFS_TYPES.contains(&obfs_type) {
            return Err(format!("不支持的 Hysteria2 混淆类型: {}", obfs_type));
        }

        Ok(())
    }

    pub fn sni(sni: &str) -> Result<(), String> {
        if sni.is_empty() {
            return Ok(());
        }

        if sni.len() > 253 {
            return Err("SNI 过长".to_string());
        }

        Self::server_address(sni)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_subscription_urls() {
        assert!(Validator::subscription_url("https://example.com/sub").is_ok());
        assert!(Validator::subscription_url("http://localhost:8080/sub").is_ok());
        assert!(
            Validator::subscription_url("https://sub.example.com:443/path?token=abc123").is_ok()
        );
    }

    #[test]
    fn test_invalid_subscription_urls() {
        assert!(Validator::subscription_url("").is_err());
        assert!(Validator::subscription_url("ftp://example.com/sub").is_err());
        assert!(Validator::subscription_url("javascript:alert(1)").is_err());
        assert!(Validator::subscription_url("not-a-url").is_err());
    }

    #[test]
    fn test_valid_node_tags() {
        assert!(Validator::node_tag("my-node").is_ok());
        assert!(Validator::node_tag("Node_123").is_ok());
        assert!(Validator::node_tag("My Node").is_ok());
        assert!(Validator::node_tag("a").is_ok());
        assert!(Validator::node_tag("香港节点").is_ok());
        assert!(Validator::node_tag("日本サーバー").is_ok());
        assert!(Validator::node_tag("节点 01-日本").is_ok());
    }

    #[test]
    fn test_invalid_node_tags() {
        assert!(Validator::node_tag("").is_err());
        assert!(Validator::node_tag(&"a".repeat(65)).is_err());
        assert!(Validator::node_tag(&"节".repeat(65)).is_err());
        assert!(Validator::node_tag("node<script>").is_err());
    }

    #[test]
    fn test_valid_server_addresses() {
        assert!(Validator::server_address("example.com").is_ok());
        assert!(Validator::server_address("sub.example.com").is_ok());
        assert!(Validator::server_address("192.168.1.1").is_ok());
        assert!(Validator::server_address("10.0.0.1").is_ok());
        assert!(Validator::server_address("example.com.").is_ok()); // FQDN with trailing dot
        assert!(Validator::server_address("::1").is_ok()); // IPv6 localhost
        assert!(Validator::server_address("2001:db8::1").is_ok()); // IPv6
    }

    #[test]
    fn test_invalid_server_addresses() {
        assert!(Validator::server_address("").is_err());
        assert!(Validator::server_address("invalid").is_err());
        assert!(Validator::server_address("-example.com").is_err());
        assert!(Validator::server_address("example-.com").is_err());
        assert!(Validator::server_address("exam ple.com").is_err()); // spaces not allowed
        assert!(Validator::server_address("example..com").is_err()); // consecutive dots
    }

    #[test]
    fn test_cipher_validation() {
        // Valid ciphers
        assert!(Validator::cipher("aes-128-gcm").is_ok());
        assert!(Validator::cipher("2022-blake3-aes-256-gcm").is_ok());

        // Invalid ciphers
        assert!(Validator::cipher("invalid-cipher").is_err());
        assert!(Validator::cipher("").is_err());
    }

    #[test]
    fn test_hysteria2_obfs_type_validation() {
        assert!(Validator::hysteria2_obfs_type("salamander").is_ok());
        assert!(Validator::hysteria2_obfs_type("gecko").is_ok());
        assert!(Validator::hysteria2_obfs_type("invalid").is_err());
    }

    #[test]
    fn test_hysteria2_obfs_request_validation() {
        let valid = NodeRequest {
            node_type: Some("hysteria2".to_string()),
            tag: "hy2".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            password: "password123".to_string(),
            obfs_type: Some("salamander".to_string()),
            obfs_password: Some("obfs-secret".to_string()),
            ..NodeRequest::default()
        };
        assert!(Validator::validate_node_request(&valid).is_ok());

        let mut missing_password = valid;
        missing_password.obfs_password = Some(" ".to_string());
        assert!(Validator::validate_node_request(&missing_password).is_err());
    }

    #[test]
    fn test_non_hysteria2_rejects_obfs_request() {
        let req = NodeRequest {
            node_type: Some("anytls".to_string()),
            tag: "anytls".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            password: "password123".to_string(),
            obfs_type: Some("salamander".to_string()),
            obfs_password: Some("obfs-secret".to_string()),
            ..NodeRequest::default()
        };

        assert!(Validator::validate_node_request(&req).is_err());
    }

    #[test]
    fn test_vmess_and_vless_require_uuid_without_password() {
        let base = NodeRequest {
            node_type: Some("vmess".to_string()),
            tag: "vmess".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            uuid: Some("123e4567-e89b-12d3-a456-426614174000".to_string()),
            cipher: Some("auto".to_string()),
            ..NodeRequest::default()
        };

        assert!(Validator::validate_node_request(&base).is_ok());

        let mut missing_uuid = base;
        missing_uuid.uuid = None;
        assert!(Validator::validate_node_request(&missing_uuid).is_err());

        let vless = NodeRequest {
            node_type: Some("vless".to_string()),
            tag: "vless".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            uuid: Some("123e4567-e89b-12d3-a456-426614174000".to_string()),
            flow: Some("xtls-rprx-vision".to_string()),
            packet_encoding: Some("xudp".to_string()),
            ..NodeRequest::default()
        };

        assert!(Validator::validate_node_request(&vless).is_ok());
    }

    #[test]
    fn test_vless_reality_requires_utls_fingerprint() {
        let mut req = NodeRequest {
            node_type: Some("vless".to_string()),
            tag: "vless-reality".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            uuid: Some("123e4567-e89b-12d3-a456-426614174000".to_string()),
            reality_public_key: Some("public-key".to_string()),
            ..NodeRequest::default()
        };

        let err = Validator::validate_node_request(&req).unwrap_err();
        assert!(err.contains("uTLS"));

        req.client_fingerprint = Some("chrome".to_string());
        assert!(Validator::validate_node_request(&req).is_ok());
    }

    #[test]
    fn test_tuic_requires_uuid_and_password() {
        let req = NodeRequest {
            node_type: Some("tuic".to_string()),
            tag: "tuic".to_string(),
            server: "example.com".to_string(),
            server_port: 443,
            password: "password123".to_string(),
            uuid: Some("123e4567-e89b-12d3-a456-426614174000".to_string()),
            tuic_congestion_control: Some("bbr".to_string()),
            tuic_udp_relay_mode: Some("quic".to_string()),
            ..NodeRequest::default()
        };

        assert!(Validator::validate_node_request(&req).is_ok());
    }

    #[test]
    fn test_transport_and_tls_fingerprint_validation() {
        assert!(Validator::transport_type("ws").is_ok());
        assert!(Validator::transport_type("xhttp").is_err());
        assert!(Validator::transport_path("/ws").is_ok());
        assert!(Validator::transport_path("ws").is_err());
        assert!(Validator::client_fingerprint("chrome").is_ok());
        assert!(Validator::client_fingerprint("unknown").is_err());
    }

    #[test]
    fn test_sni_validation() {
        // Empty SNI is valid (optional)
        assert!(Validator::sni("").is_ok());

        // Valid SNI values
        assert!(Validator::sni("example.com").is_ok());

        // Invalid SNI
        assert!(Validator::sni(&"a".repeat(254)).is_err());
    }

    #[test]
    fn test_valid_ports() {
        assert!(Validator::port(80).is_ok());
        assert!(Validator::port(443).is_ok());
        assert!(Validator::port(8080).is_ok());
        assert!(Validator::port(65535).is_ok());
    }

    #[test]
    fn test_invalid_ports() {
        assert!(Validator::port(0).is_err());
    }

    #[test]
    fn test_valid_passwords() {
        assert!(Validator::password("password123").is_ok());
        assert!(Validator::password("a".repeat(8).as_str()).is_ok());
    }

    #[test]
    fn test_invalid_passwords() {
        assert!(Validator::password("").is_err());
        assert!(Validator::password("abc").is_err());
        assert!(Validator::password("secret").is_err()); // 6 字符，不够
        assert!(Validator::password(&"a".repeat(257)).is_err());
    }
}
