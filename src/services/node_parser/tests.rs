use super::*;

#[test]
fn parse_clash_proxies_extracts_valid_nodes() {
    let yaml = r#"
proxies:
  - name: hy2-node
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass-hy
    sni: hy.example.com
  - name: anytls-node
    type: anytls
    server: any.example.com
    port: 8443
    password: pass-any
    sni: any.example.com
    skip-cert-verify: true
  - name: ss-node
    type: ss
    server: ss.example.com
    port: 8388
    cipher: 2022-blake3-aes-128-gcm
    password: pass-ss
  - name: vmess-node
    type: vmess
    server: vmess.example.com
    port: 443
    uuid: 123e4567-e89b-12d3-a456-426614174000
    cipher: auto
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 4);
    assert!(result.errors.is_empty());
    assert_eq!(result.nodes[0].0, "hy2-node");
    assert_eq!(result.nodes[1].0, "anytls-node");
    assert_eq!(result.nodes[2].0, "ss-node");
    assert_eq!(result.nodes[3].0, "vmess-node");
    assert_eq!(result.nodes[3].1["type"], "vmess");
}

#[test]
fn parse_clash_proxies_skips_invalid_nodes() {
    let yaml = r#"
proxies:
  - name: valid-node
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass-hy
  - name: invalid-missing-server
    type: hysteria2
    port: 443
    password: pass-hy
  - name: invalid-zero-port
    type: hysteria2
    server: hy.example.com
    port: 0
    password: pass-hy
  - name: invalid-missing-password
    type: hysteria2
    server: hy.example.com
    port: 443
  - name: unsupported-type
    type: snell
    server: vm.example.com
    port: 443
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].0, "valid-node");
    // 3 errors: missing-server, zero-port, missing-password
    // unsupported-type (snell) is silently skipped, not reported as error
    assert_eq!(result.errors.len(), 3);

    // Verify error messages contain node names
    assert!(result
        .errors
        .iter()
        .any(|e| e.contains("invalid-missing-server")));
    assert!(result
        .errors
        .iter()
        .any(|e| e.contains("invalid-zero-port")));
    assert!(result
        .errors
        .iter()
        .any(|e| e.contains("invalid-missing-password")));
}

#[test]
fn parse_clash_proxies_returns_empty_for_missing_proxies() {
    let yaml = "mixed-port: 7890";

    let result = parse_clash_proxies(yaml).unwrap();

    assert!(result.nodes.is_empty());
    assert!(result.errors.is_empty());
}

#[test]
fn parse_clash_proxies_reports_invalid_yaml() {
    let err = parse_clash_proxies("proxies: [").unwrap_err();

    assert!(err
        .to_string()
        .contains("Failed to parse subscription YAML"));
}

#[test]
fn parse_node_json_extracts_valid_node() {
    let json = r#"{"type":"hysteria2","tag":"test-node","server":"example.com","server_port":443,"password":"secret","tls":{"enabled":true,"server_name":"sni.example.com"}}"#;

    let (info, value) = parse_node_json(json).unwrap();

    assert_eq!(info.tag, "test-node");
    assert_eq!(info.server, "example.com");
    assert_eq!(info.server_port, 443);
    assert_eq!(info.node_type, "hysteria2");
    assert_eq!(info.sni, Some("sni.example.com".to_string()));
    // 验证返回的 Value 是正确的
    assert_eq!(value["tag"], "test-node");
    assert_eq!(value["server"], "example.com");
}

#[test]
fn parse_node_json_rejects_empty_tag() {
    let json = r#"{"type":"hysteria2","tag":"","server":"example.com","server_port":443,"password":"secret"}"#;

    let err = parse_node_json(json).unwrap_err();
    assert!(err.contains("tag"));
}

#[test]
fn parse_node_json_rejects_zero_port() {
    let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":0,"password":"secret"}"#;

    let err = parse_node_json(json).unwrap_err();
    assert!(err.contains("port"));
}

#[test]
fn parse_node_json_rejects_missing_server() {
    let json = r#"{"type":"hysteria2","tag":"test","server_port":443,"password":"secret"}"#;

    let err = parse_node_json(json).unwrap_err();
    assert!(err.contains("server"));
}

#[test]
fn parse_node_json_handles_optional_sni() {
    let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#;

    let (info, _) = parse_node_json(json).unwrap();
    assert_eq!(info.sni, None);
}

#[test]
fn parse_node_json_handles_missing_tls() {
    let json = r#"{"type":"shadowsocks","tag":"test","server":"example.com","server_port":8388,"password":"secret","method":"aes-128-gcm"}"#;

    let (info, _) = parse_node_json(json).unwrap();
    assert_eq!(info.sni, None);
}

#[test]
fn parse_node_json_rejects_port_too_large() {
    let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":65536,"password":"secret"}"#;

    let err = parse_node_json(json).unwrap_err();
    assert!(err.contains("port"));
}

#[test]
fn parse_node_json_rejects_max_valid_port() {
    // 65535 should be accepted
    let json = r#"{"type":"hysteria2","tag":"test","server":"example.com","server_port":65535,"password":"secret"}"#;

    let (info, _) = parse_node_json(json).unwrap();
    assert_eq!(info.server_port, 65535);
}

#[test]
fn parse_node_json_accepts_ipv4_server() {
    let json = r#"{"type":"hysteria2","tag":"test","server":"192.168.1.1","server_port":443,"password":"secret"}"#;

    let (info, _) = parse_node_json(json).unwrap();
    assert_eq!(info.server, "192.168.1.1");
}

#[test]
fn parse_node_json_accepts_ipv6_server() {
    let json =
        r#"{"type":"hysteria2","tag":"test","server":"::1","server_port":443,"password":"secret"}"#;

    let (info, _) = parse_node_json(json).unwrap();
    assert_eq!(info.server, "::1");
}

#[test]
fn parse_clash_proxies_handles_all_supported_types() {
    let yaml = r#"
proxies:
  - name: hy2-1
    type: hysteria2
    server: hy1.example.com
    port: 443
    password: pass1
  - name: hy2-2
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: pass2
    sni: hy2.example.com
  - name: anytls-1
    type: anytls
    server: any1.example.com
    port: 8443
    password: pass3
  - name: ss-1
    type: ss
    server: ss1.example.com
    port: 8388
    cipher: aes-256-gcm
    password: pass4
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 4);
    assert!(result.errors.is_empty());

    let types: Vec<String> = result
        .nodes
        .iter()
        .map(|(_, o)| o.get("type").unwrap().as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        types,
        vec!["hysteria2", "hysteria2", "anytls", "shadowsocks"]
    );
}

#[test]
fn parse_clash_proxies_maps_extended_supported_types() {
    let yaml = r#"
proxies:
  - name: vmess-ws
    type: vmess
    server: vm.example.com
    port: 443
    uuid: 123e4567-e89b-12d3-a456-426614174000
    cipher: auto
    alterId: 0
    tls: true
    sni: vm.example.com
    client-fingerprint: chrome
    packet-encoding: xudp
    network: ws
    ws-opts:
      path: /ws
      headers:
        Host: cdn.example.com
  - name: vless-reality
    type: vless
    server: vl.example.com
    port: 443
    uuid: 223e4567-e89b-12d3-a456-426614174000
    client-fingerprint: chrome
    flow: xtls-rprx-vision
    packet-encoding: xudp
    network: grpc
    grpc-opts:
      grpc-service-name: edge
    reality-opts:
      public-key: public-key
      short-id: abcd
  - name: trojan-grpc
    type: trojan
    server: tr.example.com
    port: 443
    password: trojan-pass
    sni: tr.example.com
    network: grpc
    grpc-opts:
      grpc-service-name: trojan
  - name: tuic-v5
    type: tuic
    server: tuic.example.com
    port: 443
    uuid: 323e4567-e89b-12d3-a456-426614174000
    password: tuic-pass
    congestion-controller: bbr
    udp-relay-mode: quic
    reduce-rtt: true
    disable-sni: true
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 4);
    assert!(result.errors.is_empty());

    let vmess = &result.nodes[0].1;
    assert_eq!(vmess["type"], "vmess");
    assert_eq!(vmess["security"], "auto");
    assert_eq!(vmess["alter_id"], 0);
    assert_eq!(vmess["packet_encoding"], "xudp");
    assert_eq!(vmess["tls"]["server_name"], "vm.example.com");
    assert_eq!(vmess["tls"]["utls"]["fingerprint"], "chrome");
    assert_eq!(vmess["transport"]["type"], "ws");
    assert_eq!(vmess["transport"]["path"], "/ws");
    assert_eq!(vmess["transport"]["headers"]["Host"], "cdn.example.com");

    let vless = &result.nodes[1].1;
    assert_eq!(vless["type"], "vless");
    assert_eq!(vless["flow"], "xtls-rprx-vision");
    assert_eq!(vless["tls"]["utls"]["fingerprint"], "chrome");
    assert_eq!(vless["tls"]["reality"]["public_key"], "public-key");
    assert_eq!(vless["tls"]["reality"]["short_id"], "abcd");
    assert_eq!(vless["transport"]["type"], "grpc");
    assert_eq!(vless["transport"]["service_name"], "edge");

    let trojan = &result.nodes[2].1;
    assert_eq!(trojan["type"], "trojan");
    assert_eq!(trojan["tls"]["server_name"], "tr.example.com");
    assert_eq!(trojan["transport"]["service_name"], "trojan");

    let tuic = &result.nodes[3].1;
    assert_eq!(tuic["type"], "tuic");
    assert_eq!(tuic["congestion_control"], "bbr");
    assert_eq!(tuic["udp_relay_mode"], "quic");
    assert_eq!(tuic["zero_rtt_handshake"], true);
    assert_eq!(tuic["tls"]["disable_sni"], true);
}

#[test]
fn parse_clash_proxies_reports_unsupported_extended_variants() {
    let yaml = r#"
proxies:
  - name: vless-encryption
    type: vless
    server: vl.example.com
    port: 443
    uuid: 123e4567-e89b-12d3-a456-426614174000
    encryption: aes-128-gcm
  - name: trojan-ss-opts
    type: trojan
    server: tr.example.com
    port: 443
    password: trojan-pass
    ss-opts:
      enabled: true
  - name: tuic-token
    type: tuic
    server: tuic.example.com
    port: 443
    token: old-token
  - name: vmess-xhttp
    type: vmess
    server: vm.example.com
    port: 443
    uuid: 223e4567-e89b-12d3-a456-426614174000
    network: xhttp
  - name: vless-reality-no-fingerprint
    type: vless
    server: vl.example.com
    port: 443
    uuid: 323e4567-e89b-12d3-a456-426614174000
    reality-opts:
      public-key: public-key
  - name: vless-reality-none-fingerprint
    type: vless
    server: vl2.example.com
    port: 443
    uuid: 423e4567-e89b-12d3-a456-426614174000
    client-fingerprint: none
    reality-opts:
      public-key: public-key
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert!(result.nodes.is_empty());
    assert_eq!(result.errors.len(), 6);
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("vless-encryption")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("trojan-ss-opts")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("tuic-token")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("vmess-xhttp")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("vless-reality-no-fingerprint")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("vless-reality-none-fingerprint")));
}

#[test]
fn parse_clash_proxies_handles_empty_proxies_list() {
    let yaml = r#"
proxies: []
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert!(result.nodes.is_empty());
    assert!(result.errors.is_empty());
}

#[test]
fn parse_clash_proxies_preserves_skip_cert_verify() {
    let yaml = r#"
proxies:
  - name: test-skip-verify
    type: hysteria2
    server: test.example.com
    port: 443
    password: pass
    skip-cert-verify: true
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 1);
    let outbound = &result.nodes[0].1;
    assert_eq!(outbound["tls"]["insecure"], true);
}

#[test]
fn parse_clash_proxies_defaults_skip_cert_verify_to_false() {
    let yaml = r#"
proxies:
  - name: test-default-verify
    type: hysteria2
    server: test.example.com
    port: 443
    password: pass
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 1);
    let outbound = &result.nodes[0].1;
    assert_eq!(outbound["tls"]["insecure"], false);
}

#[test]
fn parse_clash_proxies_maps_hysteria2_obfs() {
    let yaml = r#"
proxies:
  - name: hy2-obfs
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
    obfs: salamander
    obfs-password: obfs-pass
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 1);
    assert!(result.errors.is_empty());
    let outbound = &result.nodes[0].1;
    assert_eq!(outbound["obfs"]["type"], "salamander");
    assert_eq!(outbound["obfs"]["password"], "obfs-pass");
}

#[test]
fn parse_clash_proxies_maps_hysteria2_gecko_obfs() {
    let yaml = r#"
proxies:
  - name: hy2-gecko-obfs
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
    obfs: gecko
    obfs-password: gecko-pass
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 1);
    assert!(result.errors.is_empty());
    let outbound = &result.nodes[0].1;
    assert_eq!(outbound["obfs"]["type"], "gecko");
    assert_eq!(outbound["obfs"]["password"], "gecko-pass");
}

#[test]
fn parse_clash_proxies_omits_empty_hysteria2_obfs() {
    let yaml = r#"
proxies:
  - name: hy2-no-obfs
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
    obfs: ""
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 1);
    assert!(result.errors.is_empty());
    let outbound = &result.nodes[0].1;
    assert!(outbound.get("obfs").is_none());
}

#[test]
fn parse_clash_proxies_rejects_invalid_hysteria2_obfs() {
    let yaml = r#"
proxies:
  - name: hy2-invalid-obfs
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
    obfs: unsupported
    obfs-password: obfs-pass
  - name: hy2-missing-obfs-password
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: pass
    obfs: salamander
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert!(result.nodes.is_empty());
    assert_eq!(result.errors.len(), 2);
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("hy2-invalid-obfs")));
    assert!(result
        .errors
        .iter()
        .any(|error| error.contains("hy2-missing-obfs-password")));
}

#[test]
fn parse_clash_proxies_handles_mixed_valid_and_unsupported() {
    let yaml = r#"
proxies:
  - name: valid-hy2
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
  - name: valid-vmess
    type: vmess
    server: vm.example.com
    port: 443
    uuid: 123e4567-e89b-12d3-a456-426614174000
  - name: valid-trojan
    type: trojan
    server: tr.example.com
    port: 443
    password: pass
  - name: unsupported-snell
    type: snell
    server: snell.example.com
    port: 443
  - name: valid-ss
    type: ss
    server: ss.example.com
    port: 8388
    cipher: aes-128-gcm
    password: pass
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    // Snell remains unsupported and is silently skipped.
    assert_eq!(result.nodes.len(), 4);
    assert!(result.errors.is_empty());

    let names: Vec<String> = result.nodes.iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(
        names,
        vec!["valid-hy2", "valid-vmess", "valid-trojan", "valid-ss"]
    );
}

#[test]
fn parse_clash_proxies_hysteria2_without_bandwidth_defaults() {
    // 测试：从 Clash 配置解析 Hysteria2 时不添加硬编码带宽
    let yaml = r#"
proxies:
  - name: hy2-without-bandwidth
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
    sni: hy.example.com
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert_eq!(result.nodes.len(), 1);
    let outbound = &result.nodes[0].1;
    assert_eq!(outbound["type"], "hysteria2");
    assert_eq!(outbound["tag"], "hy2-without-bandwidth");
    // 关键测试：不应包含硬编码的 up_mbps/down_mbps
    assert!(outbound.get("up_mbps").is_none() || outbound["up_mbps"].is_null());
    assert!(outbound.get("down_mbps").is_none() || outbound["down_mbps"].is_null());
}

#[test]
fn parse_node_json_rejects_empty_server() {
    let json =
        r#"{"type":"hysteria2","tag":"test","server":"","server_port":443,"password":"secret"}"#;

    let err = parse_node_json(json).unwrap_err();
    assert!(err.contains("server"));
}

#[test]
fn parse_node_json_rejects_whitespace_only_server() {
    let json =
        r#"{"type":"hysteria2","tag":"test","server":"   ","server_port":443,"password":"secret"}"#;

    let err = parse_node_json(json).unwrap_err();
    assert!(err.contains("server"));
}

#[test]
fn parse_node_json_accepts_whitespace_in_tag() {
    let json = r#"{"type":"hysteria2","tag":"My Node 1","server":"example.com","server_port":443,"password":"secret"}"#;

    let (info, _) = parse_node_json(json).unwrap();
    assert_eq!(info.tag, "My Node 1");
}

#[test]
fn parse_clash_proxies_reports_multiple_missing_fields() {
    let yaml = r#"
proxies:
  - name: missing-everything
    type: hysteria2
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    assert!(result.nodes.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("missing-everything"));
}

#[test]
fn parse_clash_proxies_handles_ss_without_cipher() {
    let yaml = r#"
proxies:
  - name: ss-no-cipher
    type: ss
    server: ss.example.com
    port: 8388
    password: pass
"#;

    let result = parse_clash_proxies(yaml).unwrap();

    // SS without cipher should be rejected
    assert!(result.nodes.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("ss-no-cipher"));
}

#[test]
fn parse_node_json_display_format() {
    let info = NodeDisplayInfo {
        tag: "Test Node".to_string(),
        server: "192.168.1.1".to_string(),
        server_port: 8388,
        node_type: "shadowsocks".to_string(),
        sni: None,
    };

    let display = format!("{}", info);
    assert_eq!(display, "Test Node (192.168.1.1:8388) [shadowsocks]");
}
