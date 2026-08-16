use regex::Regex;
use std::sync::LazyLock;

static UUID_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});
static VALID_VMESS_SECURITIES: &[&str] =
    &["auto", "none", "zero", "aes-128-gcm", "chacha20-poly1305"];
static VALID_PACKET_ENCODINGS: &[&str] = &["packetaddr", "xudp"];
static VALID_VLESS_FLOWS: &[&str] = &["xtls-rprx-vision"];
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
static VALID_TUIC_CONGESTION_CONTROLS: &[&str] = &["cubic", "new_reno", "bbr"];
static VALID_TUIC_UDP_RELAY_MODES: &[&str] = &["native", "quic"];
pub(super) fn validate_uuid(uuid: &str) -> Result<(), String> {
    if UUID_REGEX.is_match(uuid) {
        Ok(())
    } else {
        Err("invalid UUID".to_string())
    }
}

pub(super) fn validate_vmess_security(security: &str) -> Result<(), String> {
    if VALID_VMESS_SECURITIES.contains(&security) {
        Ok(())
    } else {
        Err(format!("unsupported VMess security '{}'", security))
    }
}

pub(super) fn validate_packet_encoding(packet_encoding: &str) -> Result<(), String> {
    if VALID_PACKET_ENCODINGS.contains(&packet_encoding) {
        Ok(())
    } else {
        Err(format!("unsupported packet encoding '{}'", packet_encoding))
    }
}

pub(super) fn validate_vless_flow(flow: &str) -> Result<(), String> {
    if VALID_VLESS_FLOWS.contains(&flow) {
        Ok(())
    } else {
        Err(format!("unsupported VLESS flow '{}'", flow))
    }
}

pub(super) fn validate_client_fingerprint(fingerprint: &str) -> Result<(), String> {
    if VALID_CLIENT_FINGERPRINTS.contains(&fingerprint) {
        Ok(())
    } else {
        Err(format!(
            "unsupported TLS client fingerprint '{}'",
            fingerprint
        ))
    }
}

pub(super) fn validate_tuic_congestion_control(congestion_control: &str) -> Result<(), String> {
    if VALID_TUIC_CONGESTION_CONTROLS.contains(&congestion_control) {
        Ok(())
    } else {
        Err(format!(
            "unsupported TUIC congestion controller '{}'",
            congestion_control
        ))
    }
}

pub(super) fn validate_tuic_udp_relay_mode(udp_relay_mode: &str) -> Result<(), String> {
    if VALID_TUIC_UDP_RELAY_MODES.contains(&udp_relay_mode) {
        Ok(())
    } else {
        Err(format!(
            "unsupported TUIC UDP relay mode '{}'",
            udp_relay_mode
        ))
    }
}
