use crate::error::{AppError, AppResult};
use crate::services::node_parser::parse_clash_proxies;

/// 订阅获取结果，包含节点和解析错误信息
pub struct FetchResult {
    pub node_names: Vec<String>,
    pub outbounds: Vec<serde_json::Value>,
    pub parse_errors: Vec<String>,
    pub total_count: usize,
}

pub async fn fetch_sub(link: &str, client: &reqwest::Client) -> AppResult<FetchResult> {
    let res = client
        .get(link)
        .timeout(std::time::Duration::from_secs(30))
        .header("User-Agent", "clash-meta")
        .send()
        .await
        .map_err(|e| AppError::context(format!("Failed to fetch subscription from {}", link), e))?;

    let text = res
        .text()
        .await
        .map_err(|e| AppError::context(format!("Failed to read subscription response from {}", link), e))?;

    let parse_result = parse_clash_proxies(&text)
        .map_err(|e| AppError::context(format!("Failed to parse subscription content from {}", link), e))?;

    let total_count = parse_result.total_count;
    let node_names: Vec<String> = parse_result.nodes.iter().map(|(n, _)| n.clone()).collect();
    let outbounds: Vec<serde_json::Value> = parse_result.nodes.into_iter().map(|(_, o)| o).collect();

    // 解析错误将由调用方统一处理，此处不再打印

    Ok(FetchResult {
        node_names,
        outbounds,
        parse_errors: parse_result.errors,
        total_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clash_proxies_extracts_supported_nodes() {
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
  - name: ignored-node
    type: vmess
    server: vmess.example.com
    port: 443
    uuid: xxx
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        // 3 valid nodes + 1 unsupported type (vmess) silently skipped
        let names: Vec<String> = result.nodes.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(names, vec!["hy2-node", "anytls-node", "ss-node"]);
        assert_eq!(result.nodes.len(), 3);
        assert!(result.errors.is_empty()); // vmess is silently skipped, not reported as error

        let outbounds: Vec<serde_json::Value> = result.nodes.into_iter().map(|(_, o)| o).collect();
        assert_eq!(outbounds[0]["type"], "hysteria2");
        assert_eq!(outbounds[0]["tag"], "hy2-node");
        assert_eq!(outbounds[0]["tls"]["server_name"], "hy.example.com");
        assert_eq!(outbounds[1]["type"], "anytls");
        assert_eq!(outbounds[1]["tls"]["insecure"], true);
        assert_eq!(outbounds[2]["type"], "shadowsocks");
        assert_eq!(outbounds[2]["method"], "2022-blake3-aes-128-gcm");
    }

    #[test]
    fn parse_clash_proxies_skips_invalid_nodes_with_errors() {
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
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.errors.len(), 2);
        assert!(result.errors.iter().any(|e| e.contains("invalid-missing-server")));
        assert!(result.errors.iter().any(|e| e.contains("invalid-zero-port")));
    }

    #[test]
    fn parse_clash_proxies_returns_empty_when_proxies_missing() {
        let yaml = "mixed-port: 7890";

        let result = parse_clash_proxies(yaml).unwrap();

        assert!(result.nodes.is_empty());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn parse_clash_proxies_reports_invalid_yaml() {
        let err = parse_clash_proxies("proxies: [").unwrap_err();

        assert!(err.to_string().contains("Failed to parse subscription YAML"));
    }

    #[test]
    fn parse_clash_proxies_preserves_node_order() {
        let yaml = r#"
proxies:
  - name: first
    type: hysteria2
    server: first.example.com
    port: 443
    password: pass
  - name: second
    type: anytls
    server: second.example.com
    port: 8443
    password: pass
  - name: third
    type: ss
    server: third.example.com
    port: 8388
    cipher: aes-128-gcm
    password: pass
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        let names: Vec<String> = result.nodes.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    #[test]
    fn parse_clash_proxies_handles_duplicate_names() {
        let yaml = r#"
proxies:
  - name: duplicate-name
    type: hysteria2
    server: hy1.example.com
    port: 443
    password: pass1
  - name: duplicate-name
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: pass2
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        // Both nodes should be parsed (sing-box will handle duplicate tags)
        assert_eq!(result.nodes.len(), 2);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn parse_clash_proxies_handles_unicode_in_names() {
        let yaml = r#"
proxies:
  - name: "节点-测试"
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].0, "节点-测试");
    }

    #[test]
    fn parse_clash_proxies_handles_very_long_node_names() {
        let long_name = "a".repeat(200);
        let yaml = format!(r#"
proxies:
  - name: "{}"
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
"#, long_name);

        let result = parse_clash_proxies(&yaml).unwrap();

        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].0, long_name);
    }

    #[test]
    fn parse_clash_proxies_handles_nodes_without_names() {
        let yaml = r#"
proxies:
  - type: hysteria2
    server: hy1.example.com
    port: 443
    password: pass1
  - name: named-node
    type: hysteria2
    server: hy2.example.com
    port: 443
    password: pass2
"#;

        let result = parse_clash_proxies(yaml).unwrap();

        // First node should be reported with index-based name in error
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("<index 0>"));
    }
}
