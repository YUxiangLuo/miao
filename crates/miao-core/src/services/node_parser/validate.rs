use crate::validation::{
    UUID_REGEX, VALID_CLIENT_FINGERPRINTS, VALID_PACKET_ENCODINGS, VALID_TUIC_CONGESTION_CONTROLS,
    VALID_TUIC_UDP_RELAY_MODES, VALID_VLESS_FLOWS, VALID_VMESS_CIPHERS,
};

pub(super) fn validate_uuid(uuid: &str) -> Result<(), String> {
    if UUID_REGEX.is_match(uuid) {
        Ok(())
    } else {
        Err("invalid UUID".to_string())
    }
}

pub(super) fn validate_vmess_security(security: &str) -> Result<(), String> {
    if VALID_VMESS_CIPHERS.contains(&security) {
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
