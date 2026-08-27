use super::*;

#[test]
fn custom_rule_accepts_valid_entries() {
    let ok = |field: &str, value: &str| {
        Validator::custom_rule(
            &RuleRequest {
                field: field.to_string(),
                value: value.to_string(),
                target: "direct".to_string(),
            },
            &[],
        )
    };
    assert!(ok("domain_suffix", "example.com").is_ok());
    assert!(ok("domain_keyword", "google").is_ok());
    assert!(ok("process_name", "curl").is_ok());
    assert!(ok("process_path", "/usr/bin/curl").is_ok());
    assert!(ok("port", "443").is_ok());
    assert!(ok("port_range", "1000:2000").is_ok());
    assert!(ok("port_range", ":3000").is_ok());
    assert!(ok("port_range", "4000:").is_ok());
    assert!(ok("protocol", "quic").is_ok());
    assert!(ok("protocol", "tls").is_ok());
    assert!(ok("ip_cidr", "192.168.0.0/16").is_ok());
    assert!(ok("ip_cidr", "2001:db8::/32").is_ok());
    assert!(ok("source_ip_cidr", "192.168.1.0/24").is_ok());
}

#[test]
fn custom_rule_rejects_invalid_entries() {
    let err = |field: &str, value: &str, target: &str| {
        Validator::custom_rule(
            &RuleRequest {
                field: field.to_string(),
                value: value.to_string(),
                target: target.to_string(),
            },
            &[],
        )
    };
    assert!(err("rule_set", "x", "proxy").is_err());
    assert!(err("domain", "example.com", "block").is_err());
    assert!(err("port", "0", "proxy").is_err());
    assert!(err("port", "70000", "proxy").is_err());
    assert!(err("port", "abc", "proxy").is_err());
    assert!(err("ip_cidr", "192.168.0.0", "proxy").is_err());
    assert!(err("ip_cidr", "192.168.0.0/33", "proxy").is_err());
    assert!(err("ip_cidr", "2001:db8::/129", "proxy").is_err());
    assert!(err("source_ip_cidr", "192.168.1.0", "proxy").is_err());
    assert!(err("port_range", "1000", "proxy").is_err());
    assert!(err("port_range", ":", "proxy").is_err());
    assert!(err("port_range", "2000:1000", "proxy").is_err());
    assert!(err("port_range", "0:100", "proxy").is_err());
    assert!(err("protocol", "ftp", "proxy").is_err());
    assert!(err("protocol", "QUIC", "proxy").is_err());
    assert!(err("domain_suffix", "exa mple.com", "proxy").is_err());
    assert!(err("domain_suffix", "", "proxy").is_err());
}

#[test]
fn custom_rule_accepts_known_node_tag_as_target() {
    let nodes = vec!["香港节点".to_string(), "hy2-us".to_string()];
    let req = RuleRequest {
        field: "process_name".to_string(),
        value: "curl".to_string(),
        target: "香港节点".to_string(),
    };
    assert!(Validator::custom_rule(&req, &nodes).is_ok());
}

#[test]
fn custom_rule_rejects_unknown_node_tag_as_target() {
    let nodes = vec!["香港节点".to_string()];
    let req = RuleRequest {
        field: "process_path".to_string(),
        value: "/usr/bin/curl".to_string(),
        target: "不存在的节点".to_string(),
    };
    let err = Validator::custom_rule(&req, &nodes).unwrap_err();
    assert!(err.contains("不支持的规则目标"));
}

#[test]
fn test_valid_subscription_urls() {
    assert!(Validator::subscription_url("https://example.com/sub").is_ok());
    assert!(Validator::subscription_url("http://localhost:8080/sub").is_ok());
    assert!(Validator::subscription_url("https://sub.example.com:443/path?token=abc123").is_ok());
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
    // 首尾空白允许(存储时会 trim),仅纯空白拒绝
    assert!(Validator::node_tag(" my-node ").is_ok());
}

#[test]
fn test_invalid_node_tags() {
    assert!(Validator::node_tag("").is_err());
    // 纯空白(trim 后为空)同样拒绝,否则会落盘成空串节点
    assert!(Validator::node_tag("   ").is_err());
    assert!(Validator::node_tag(" \t ").is_err());
    assert!(Validator::node_tag(&"a".repeat(65)).is_err());
    assert!(Validator::node_tag(&"节".repeat(65)).is_err());
    assert!(Validator::node_tag("node<script>").is_err());
    // 保留字(内置出站 proxy/direct、拦截动作 reject),大小写与首尾空白不敏感
    assert!(Validator::node_tag("proxy").is_err());
    assert!(Validator::node_tag("direct").is_err());
    assert!(Validator::node_tag("reject").is_err());
    assert!(Validator::node_tag("Proxy").is_err());
    assert!(Validator::node_tag(" DIRECT ").is_err());
}

#[test]
fn test_node_tags_allow_reserved_substrings() {
    assert!(Validator::node_tag("my-proxy").is_ok());
    assert!(Validator::node_tag("proxy-01").is_ok());
    assert!(Validator::node_tag("director").is_ok());
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
        password: Some("password123".to_string()),
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
        password: Some("password123".to_string()),
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
        password: Some("password123".to_string()),
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
