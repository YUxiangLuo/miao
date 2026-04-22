use regex::Regex;
use std::sync::LazyLock;

static VALID_TAG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9\-_\s]{1,64}$").unwrap());

static VALID_CIPHERS: &[&str] = &[
    "2022-blake3-aes-128-gcm",
    "2022-blake3-aes-256-gcm",
    "2022-blake3-chacha20-poly1305",
    "aes-128-gcm",
    "aes-256-gcm",
    "chacha20-ietf-poly1305",
];

pub static VALID_VMESS_SECURITY: &[&str] = &[
    "auto",
    "none",
    "zero",
    "aes-128-gcm",
    "chacha20-poly1305",
    "aes-128-ctr",
];

use crate::models::NodeRequest;

pub struct Validator;

impl Validator {
    pub fn validate_node_request(req: &NodeRequest) -> Result<(), String> {
        Self::node_tag(&req.tag)?;
        Self::server_address(&req.server)?;
        Self::port(req.server_port)?;
        Self::password(&req.password)?;
        if let Some(ref sni) = req.sni {
            Self::sni(sni)?;
        }
        if let Some(ref cipher) = req.cipher {
            Self::cipher(cipher)?;
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

        if tag.len() > 64 {
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
        if !VALID_CIPHERS.contains(&cipher) {
            return Err(format!("不支持的加密方式: {}", cipher));
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
    }

    #[test]
    fn test_invalid_node_tags() {
        assert!(Validator::node_tag("").is_err());
        assert!(Validator::node_tag(&"a".repeat(65)).is_err());
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

    #[test]
    fn test_valid_vmess_security_values() {
        // auto, none, zero, aes-128-gcm, chacha20-poly1305, aes-128-ctr
        assert!(VALID_VMESS_SECURITY.contains(&"auto"));
        assert!(VALID_VMESS_SECURITY.contains(&"none"));
        assert!(VALID_VMESS_SECURITY.contains(&"zero"));
        assert!(VALID_VMESS_SECURITY.contains(&"aes-128-gcm"));
        assert!(VALID_VMESS_SECURITY.contains(&"chacha20-poly1305"));
        assert!(VALID_VMESS_SECURITY.contains(&"aes-128-ctr"));
        assert!(!VALID_VMESS_SECURITY.contains(&"invalid"));
    }
}
