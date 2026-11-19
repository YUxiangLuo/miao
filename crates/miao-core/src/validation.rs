use regex::Regex;
use std::sync::LazyLock;

static VALID_TAG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}\-_\s]{1,64}$").unwrap());
pub(crate) static UUID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});

pub(crate) static VALID_NODE_TYPES: &[&str] = &[
    "hysteria2",
    "anytls",
    "ss",
    "socks",
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

pub(crate) static VALID_VMESS_CIPHERS: &[&str] =
    &["auto", "none", "zero", "aes-128-gcm", "chacha20-poly1305"];
static VALID_TRANSPORT_TYPES: &[&str] = &["tcp", "ws", "http", "h2", "grpc"];
pub(crate) static VALID_CLIENT_FINGERPRINTS: &[&str] = &[
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
pub(crate) static VALID_PACKET_ENCODINGS: &[&str] = &["packetaddr", "xudp"];
pub(crate) static VALID_VLESS_FLOWS: &[&str] = &["xtls-rprx-vision"];
pub(crate) static VALID_TUIC_CONGESTION_CONTROLS: &[&str] = &["cubic", "new_reno", "bbr"];
pub(crate) static VALID_TUIC_UDP_RELAY_MODES: &[&str] = &["native", "quic"];
static VALID_HYSTERIA2_OBFS_TYPES: &[&str] = &["salamander", "gecko"];

/// Canonical fields accepted by the structured custom-rule API. Raw rules in
/// existing config files remain forward-compatible and may contain more
/// sing-box matchers.
pub const CUSTOM_RULE_FIELDS: &[&str] = &[
    "domain_suffix",
    "domain",
    "domain_keyword",
    "ip_cidr",
    "source_ip_cidr",
    "port",
    "port_range",
    "protocol",
    "process_name",
    "process_path",
];

use crate::models::{NodeRequest, RuleRequest};

pub struct Validator;

pub(crate) fn non_empty(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RequestProtocol {
    Hysteria2,
    Anytls,
    Shadowsocks,
    Socks5,
    Vmess,
    Vless,
    Trojan,
    Tuic,
}

impl RequestProtocol {
    fn parse(node_type: &str) -> Result<Self, String> {
        match node_type {
            "hysteria2" => Ok(Self::Hysteria2),
            "anytls" => Ok(Self::Anytls),
            "ss" => Ok(Self::Shadowsocks),
            "socks" => Ok(Self::Socks5),
            "vmess" => Ok(Self::Vmess),
            "vless" => Ok(Self::Vless),
            "trojan" => Ok(Self::Trojan),
            "tuic" => Ok(Self::Tuic),
            _ => Err(format!(
                "不支持的节点类型: {}，支持的类型: {}",
                node_type,
                VALID_NODE_TYPES.join(", ")
            )),
        }
    }

    fn requires_password(self) -> bool {
        matches!(
            self,
            Self::Hysteria2 | Self::Anytls | Self::Shadowsocks | Self::Trojan | Self::Tuic
        )
    }

    fn requires_uuid(self) -> bool {
        matches!(self, Self::Vmess | Self::Vless | Self::Tuic)
    }

    fn supports_v2ray_transport(self) -> bool {
        matches!(self, Self::Vmess | Self::Vless | Self::Trojan)
    }

    fn supports_packet_encoding(self) -> bool {
        matches!(self, Self::Vmess | Self::Vless)
    }
}

fn ensure_member(value: &str, allowed: &[&str], label: &str) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("不支持的{label}: {value}"))
    }
}

fn has_socks5_tls_fields(req: &NodeRequest) -> bool {
    req.tls_enabled == Some(true)
        || non_empty(&req.sni).is_some()
        || req.skip_cert_verify == Some(true)
        || non_empty(&req.client_fingerprint).is_some()
        || req.alpn.as_ref().is_some_and(|items| !items.is_empty())
}

fn validate_port_range(value: &str) -> Result<(), String> {
    // sing-box 格式: 1000:2000 / :3000 / 4000:
    let Some((start, end)) = value.split_once(':') else {
        return Err("端口范围必须形如 1000:2000（可省略一端）".to_string());
    };
    let valid_side = |side: &str| side.is_empty() || side.parse::<u16>().is_ok_and(|port| port > 0);
    if (start.is_empty() && end.is_empty()) || !valid_side(start) || !valid_side(end) {
        return Err("端口范围必须形如 1000:2000（可省略一端）,端口为 1-65535".to_string());
    }
    if let (Ok(s), Ok(e)) = (start.parse::<u16>(), end.parse::<u16>()) {
        if s > e {
            return Err("端口范围起点不能大于终点".to_string());
        }
    }
    Ok(())
}

fn validate_ip_cidr(value: &str) -> Result<(), String> {
    let Some((addr, prefix)) = value.split_once('/') else {
        return Err("IP/CIDR 必须形如 192.168.0.0/16".to_string());
    };
    let Ok(ip) = addr.parse::<std::net::IpAddr>() else {
        return Err("无效的 IP/CIDR".to_string());
    };
    let max_prefix = if ip.is_ipv4() { 32 } else { 128 };
    if !prefix.parse::<u8>().is_ok_and(|p| p <= max_prefix) {
        return Err(format!("前缀长度必须在 0-{} 之间", max_prefix));
    }
    Ok(())
}

fn validate_domain_label(part: &str) -> Result<(), String> {
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
    Ok(())
}

impl Validator {
    pub fn validate_node_request(req: &NodeRequest) -> Result<(), String> {
        Self::node_tag(&req.tag)?;
        Self::server_address(&req.server)?;
        Self::port(req.server_port)?;
        let node_type = req.node_type.as_deref().unwrap_or("hysteria2");
        Self::node_type(node_type)?;
        let protocol = RequestProtocol::parse(node_type)?;

        Self::validate_protocol_credentials(protocol, req)?;
        if let Some(ref sni) = req.sni {
            Self::sni(sni)?;
        }
        Self::validate_node_cipher(protocol, node_type, req)?;
        Self::validate_transport_options(protocol, node_type, req)?;
        Self::validate_tls_and_reality_options(protocol, req)?;
        Self::validate_protocol_extensions(protocol, req)?;
        Ok(())
    }

    fn validate_protocol_credentials(
        protocol: RequestProtocol,
        req: &NodeRequest,
    ) -> Result<(), String> {
        if protocol == RequestProtocol::Socks5 {
            Self::socks5_auth(
                req.username.as_deref().unwrap_or_default(),
                req.password.as_deref().unwrap_or_default(),
            )?;
            if has_socks5_tls_fields(req) {
                return Err("SOCKS5 节点不支持 TLS 配置".to_string());
            }
        }
        if protocol.requires_password() {
            Self::password(req.password.as_deref().unwrap_or_default())?;
        }
        if protocol.requires_uuid() {
            let uuid = non_empty(&req.uuid).ok_or("UUID 不能为空")?;
            Self::uuid(uuid)?;
        }
        Ok(())
    }

    fn validate_node_cipher(
        protocol: RequestProtocol,
        node_type: &str,
        req: &NodeRequest,
    ) -> Result<(), String> {
        let Some(cipher) = req.cipher.as_deref().filter(|c| !c.trim().is_empty()) else {
            return Ok(());
        };
        match protocol {
            RequestProtocol::Vmess => Self::vmess_cipher(cipher),
            RequestProtocol::Shadowsocks => Self::cipher(cipher),
            _ => Err(format!("{} 节点不支持加密方式字段", node_type)),
        }
    }

    fn validate_transport_options(
        protocol: RequestProtocol,
        node_type: &str,
        req: &NodeRequest,
    ) -> Result<(), String> {
        if let Some(transport_type) = non_empty(&req.transport_type) {
            Self::transport_type(transport_type)?;
            if !protocol.supports_v2ray_transport() {
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
        Ok(())
    }

    fn validate_tls_and_reality_options(
        protocol: RequestProtocol,
        req: &NodeRequest,
    ) -> Result<(), String> {
        if let Some(ref alpn) = req.alpn {
            Self::alpn(alpn)?;
        }
        if let Some(fingerprint) = non_empty(&req.client_fingerprint) {
            Self::client_fingerprint(fingerprint)?;
        }
        if non_empty(&req.reality_public_key).is_some()
            || non_empty(&req.reality_short_id).is_some()
        {
            if protocol != RequestProtocol::Vless {
                return Err("只有 VLESS 节点支持 Reality 配置".to_string());
            }
            if non_empty(&req.reality_public_key).is_none() {
                return Err("Reality public key 不能为空".to_string());
            }
            let fingerprint = non_empty(&req.client_fingerprint)
                .ok_or("Reality 节点必须配置 TLS 指纹（uTLS）")?;
            Self::client_fingerprint(fingerprint)?;
        }
        Ok(())
    }

    fn validate_protocol_extensions(
        protocol: RequestProtocol,
        req: &NodeRequest,
    ) -> Result<(), String> {
        if let Some(flow) = non_empty(&req.flow) {
            if protocol != RequestProtocol::Vless {
                return Err("只有 VLESS 节点支持 flow 字段".to_string());
            }
            Self::vless_flow(flow)?;
        }
        if let Some(packet_encoding) = non_empty(&req.packet_encoding) {
            if !protocol.supports_packet_encoding() {
                return Err("只有 VMess/VLESS 节点支持 packet encoding".to_string());
            }
            Self::packet_encoding(packet_encoding)?;
        }
        if let Some(congestion_control) = non_empty(&req.tuic_congestion_control) {
            if protocol != RequestProtocol::Tuic {
                return Err("只有 TUIC 节点支持拥塞控制配置".to_string());
            }
            Self::tuic_congestion_control(congestion_control)?;
        }
        if let Some(udp_relay_mode) = non_empty(&req.tuic_udp_relay_mode) {
            if protocol != RequestProtocol::Tuic {
                return Err("只有 TUIC 节点支持 UDP relay mode".to_string());
            }
            Self::tuic_udp_relay_mode(udp_relay_mode)?;
        }
        Self::validate_hysteria2_obfs_options(protocol, req)
    }

    fn validate_hysteria2_obfs_options(
        protocol: RequestProtocol,
        req: &NodeRequest,
    ) -> Result<(), String> {
        let has_obfs_password = non_empty(&req.obfs_password).is_some();
        if protocol != RequestProtocol::Hysteria2 && (req.obfs_type.is_some() || has_obfs_password)
        {
            return Err("只有 Hysteria2 节点支持混淆配置".to_string());
        }
        match req.obfs_type.as_deref() {
            Some(obfs_type) => {
                Self::hysteria2_obfs_type(obfs_type)?;
                let password = non_empty(&req.obfs_password).ok_or("混淆密码不能为空")?;
                if password.len() > 256 {
                    return Err("混淆密码过长（最多 256 个字符）".to_string());
                }
                Ok(())
            }
            None if has_obfs_password => Err("请先选择混淆类型".to_string()),
            None => Ok(()),
        }
    }

    pub fn subscription_url(url: &str) -> Result<(), String> {
        if url.is_empty() {
            return Err("订阅链接不能为空".to_string());
        }
        if url.len() > 4096 {
            return Err("订阅链接过长".to_string());
        }

        let parsed = url::Url::parse(url).map_err(|_| "无效的订阅链接格式".to_string())?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("订阅链接必须使用 HTTP 或 HTTPS 协议".to_string());
        }
        if parsed.host_str().is_none() {
            return Err("订阅链接缺少有效的主机名".to_string());
        }
        Ok(())
    }

    pub fn node_tag(tag: &str) -> Result<(), String> {
        // 存储时会 trim(见 services::commands::nodes),这里先 trim 再判空,
        // 否则纯空白名称能通过校验、落盘成空串,UI 不可见也无法删除
        let tag = tag.trim();
        if tag.is_empty() {
            return Err("节点名称不能为空".to_string());
        }
        if tag.chars().count() > 64 {
            return Err("节点名称不能超过 64 个字符".to_string());
        }
        if !VALID_TAG_REGEX.is_match(tag) {
            return Err("节点名称只能包含字母、数字、空格、下划线和连字符".to_string());
        }

        // 与内置出站/规则动作同名会让运行时的实际指向与面板显示不一致:
        // proxy/direct 是模板内置出站(builder.rs),reject 是拦截动作
        const RESERVED_TAGS: &[&str] = &["proxy", "direct", "reject"];
        if RESERVED_TAGS.contains(&tag.to_lowercase().as_str()) {
            return Err("节点名称不能使用保留字 proxy / direct / reject".to_string());
        }

        Ok(())
    }

    /// 校验自定义规则的字段、目标与取值格式;extra_targets 为内置目标之外可用的节点 tag
    pub fn custom_rule(req: &RuleRequest, extra_targets: &[String]) -> Result<(), String> {
        static VALID_RULE_TARGETS: &[&str] = &["proxy", "direct", "reject"];
        static VALID_PROTOCOLS: &[&str] = &[
            "http",
            "tls",
            "quic",
            "stun",
            "dns",
            "bittorrent",
            "dtls",
            "ssh",
            "rdp",
            "ntp",
        ];

        if !CUSTOM_RULE_FIELDS.contains(&req.field.as_str()) {
            return Err(format!(
                "不支持的规则字段: {},支持的字段: {}",
                req.field,
                CUSTOM_RULE_FIELDS.join(", ")
            ));
        }
        let target_known = VALID_RULE_TARGETS.contains(&req.target.as_str())
            || extra_targets.iter().any(|tag| tag == &req.target);
        if !target_known {
            return Err(format!(
                "不支持的规则目标: {},支持的目标: {} 或已存在的节点名",
                req.target,
                VALID_RULE_TARGETS.join(", ")
            ));
        }

        let value = req.value.trim();
        if value.is_empty() {
            return Err("规则值不能为空".to_string());
        }
        if value.chars().count() > 256 {
            return Err("规则值不能超过 256 个字符".to_string());
        }

        match req.field.as_str() {
            "port" => {
                if !value.parse::<u16>().is_ok_and(|port| port > 0) {
                    return Err("端口必须是 1-65535 的整数".to_string());
                }
            }
            "port_range" => validate_port_range(value)?,
            "protocol" => {
                if !VALID_PROTOCOLS.contains(&value) {
                    return Err(format!(
                        "不支持的嗅探协议: {},支持: {}",
                        value,
                        VALID_PROTOCOLS.join(", ")
                    ));
                }
            }
            "ip_cidr" | "source_ip_cidr" => validate_ip_cidr(value)?,
            "domain" | "domain_suffix" | "domain_keyword"
                if value.contains('/')
                    || value.contains(':')
                    || value.contains(char::is_whitespace) =>
            {
                return Err("域名规则值不能包含 / : 或空白字符".to_string());
            }
            _ => {}
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

        for part in server.split('.') {
            validate_domain_label(part)?;
        }
        Ok(())
    }

    pub fn port(port: u16) -> Result<(), String> {
        if port == 0 {
            Err("端口号不能为 0".to_string())
        } else {
            Ok(())
        }
    }

    pub fn node_type(node_type: &str) -> Result<(), String> {
        RequestProtocol::parse(node_type).map(|_| ())
    }

    pub fn uuid(uuid: &str) -> Result<(), String> {
        if UUID_REGEX.is_match(uuid) {
            Ok(())
        } else {
            Err("UUID 格式无效".to_string())
        }
    }

    pub fn socks5_auth(username: &str, password: &str) -> Result<(), String> {
        if username.is_empty() && password.is_empty() {
            return Ok(());
        }
        if username.is_empty() || password.is_empty() {
            return Err("SOCKS5 用户名和密码须同时填写，或同时留空".to_string());
        }
        if username.len() > 255 || password.len() > 255 {
            return Err("SOCKS5 用户名和密码各不能超过 255 字节".to_string());
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
        ensure_member(cipher, VALID_SS_CIPHERS, "加密方式")
    }

    pub fn vmess_cipher(cipher: &str) -> Result<(), String> {
        ensure_member(cipher, VALID_VMESS_CIPHERS, " VMess 加密方式")
    }

    pub fn transport_type(transport_type: &str) -> Result<(), String> {
        ensure_member(transport_type, VALID_TRANSPORT_TYPES, "传输层类型")
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
            Err("gRPC service name 过长".to_string())
        } else {
            Ok(())
        }
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
        ensure_member(packet_encoding, VALID_PACKET_ENCODINGS, " packet encoding")
    }

    pub fn vless_flow(flow: &str) -> Result<(), String> {
        ensure_member(flow, VALID_VLESS_FLOWS, " VLESS flow")
    }

    pub fn tuic_congestion_control(congestion_control: &str) -> Result<(), String> {
        ensure_member(
            congestion_control,
            VALID_TUIC_CONGESTION_CONTROLS,
            " TUIC 拥塞控制",
        )
    }

    pub fn tuic_udp_relay_mode(udp_relay_mode: &str) -> Result<(), String> {
        ensure_member(
            udp_relay_mode,
            VALID_TUIC_UDP_RELAY_MODES,
            " TUIC UDP relay mode",
        )
    }

    pub fn hysteria2_obfs_type(obfs_type: &str) -> Result<(), String> {
        ensure_member(obfs_type, VALID_HYSTERIA2_OBFS_TYPES, " Hysteria2 混淆类型")
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
mod tests;
