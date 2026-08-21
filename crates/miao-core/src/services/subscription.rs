use crate::error::{AppError, AppResult};
use crate::services::node_parser::parse_clash_proxies;

/// 订阅获取结果，包含节点和解析错误信息
#[derive(Debug)]
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
        .map_err(|e| AppError::context(format!("Failed to fetch subscription from {}", link), e))?
        .error_for_status()
        .map_err(|e| {
            AppError::context(
                format!("Subscription server returned HTTP error for {}", link),
                e,
            )
        })?;

    let text = res.text().await.map_err(|e| {
        AppError::context(
            format!("Failed to read subscription response from {}", link),
            e,
        )
    })?;

    let parse_result = parse_clash_proxies(&text).map_err(|e| {
        AppError::context(
            format!("Failed to parse subscription content from {}", link),
            e,
        )
    })?;

    let total_count = parse_result.total_count;
    let node_names: Vec<String> = parse_result.nodes.iter().map(|(n, _)| n.clone()).collect();
    let outbounds: Vec<serde_json::Value> =
        parse_result.nodes.into_iter().map(|(_, o)| o).collect();

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

    #[tokio::test]
    async fn fetch_sub_rejects_http_error_status() {
        use axum::{http::StatusCode, routing::get, Router};

        let app = Router::new().route(
            "/sub",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let err = fetch_sub(&format!("http://{addr}/sub"), &client)
            .await
            .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("Subscription server returned HTTP error"));
        assert!(message.contains("500"));
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

        // Both nodes should be parsed; config generation will de-duplicate tags later.
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
        let yaml = format!(
            r#"
proxies:
  - name: "{}"
    type: hysteria2
    server: hy.example.com
    port: 443
    password: pass
"#,
            long_name
        );

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
