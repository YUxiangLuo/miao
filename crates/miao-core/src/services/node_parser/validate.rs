use crate::validation::{
    UUID_REGEX, VALID_CLIENT_FINGERPRINTS, VALID_PACKET_ENCODINGS, VALID_TUIC_CONGESTION_CONTROLS,
    VALID_TUIC_UDP_RELAY_MODES, VALID_VLESS_FLOWS, VALID_VMESS_CIPHERS,
};

fn ensure_allowed(value: &str, allowed: &[&str], label: &str) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("unsupported {label} '{value}'"))
    }
}

pub(super) fn validate_uuid(uuid: &str) -> Result<(), String> {
    if UUID_REGEX.is_match(uuid) {
        Ok(())
    } else {
        Err("invalid UUID".to_string())
    }
}

pub(super) fn validate_vmess_security(security: &str) -> Result<(), String> {
    ensure_allowed(security, VALID_VMESS_CIPHERS, "VMess security")
}

pub(super) fn validate_packet_encoding(packet_encoding: &str) -> Result<(), String> {
    ensure_allowed(packet_encoding, VALID_PACKET_ENCODINGS, "packet encoding")
}

pub(super) fn validate_vless_flow(flow: &str) -> Result<(), String> {
    ensure_allowed(flow, VALID_VLESS_FLOWS, "VLESS flow")
}

pub(super) fn validate_client_fingerprint(fingerprint: &str) -> Result<(), String> {
    ensure_allowed(
        fingerprint,
        VALID_CLIENT_FINGERPRINTS,
        "TLS client fingerprint",
    )
}

pub(super) fn validate_tuic_congestion_control(congestion_control: &str) -> Result<(), String> {
    ensure_allowed(
        congestion_control,
        VALID_TUIC_CONGESTION_CONTROLS,
        "TUIC congestion controller",
    )
}

pub(super) fn validate_tuic_udp_relay_mode(udp_relay_mode: &str) -> Result<(), String> {
    ensure_allowed(
        udp_relay_mode,
        VALID_TUIC_UDP_RELAY_MODES,
        "TUIC UDP relay mode",
    )
}
