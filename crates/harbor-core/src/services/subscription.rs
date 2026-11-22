use crate::error::AppError;
use crate::models::SubscriptionFailureKind;

#[derive(Debug)]
pub struct FetchError {
    pub kind: SubscriptionFailureKind,
    pub error: AppError,
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

impl FetchError {
    fn classify_reqwest_error(error: &reqwest::Error) -> SubscriptionFailureKind {
        if error.is_timeout() {
            SubscriptionFailureKind::Timeout
        } else if error.status().is_some() {
            SubscriptionFailureKind::Http
        } else {
            SubscriptionFailureKind::Network
        }
    }

    fn request(context: String, error: reqwest::Error) -> Self {
        Self {
            kind: Self::classify_reqwest_error(&error),
            error: AppError::context(context, error),
        }
    }

    fn parse(error: AppError) -> Self {
        Self {
            kind: SubscriptionFailureKind::Parse,
            error,
        }
    }
}
use crate::services::node_parser::parse_clash_proxies;

/// 订阅获取结果，包含节点和解析错误信息
#[derive(Debug)]
pub struct FetchResult {
    pub node_names: Vec<String>,
    pub outbounds: Vec<serde_json::Value>,
    pub parse_errors: Vec<String>,
    pub total_count: usize,
    pub filtered_info_count: usize,
}

// Bound downloaded material before allocating a YAML tree, including responses
// without Content-Length. Oversized sources are failures, never empty pools.
const MAX_SUBSCRIPTION_BYTES: usize = 4 * 1024 * 1024;

fn oversized_subscription() -> FetchError {
    FetchError::parse(AppError::message("订阅内容超过 4 MiB 上限"))
}

const STRONG_INFO_NAME_MARKERS: &[&str] = &[
    "剩余流量",
    "流量剩余",
    "距离下次重置",
    "下次重置剩余",
    "套餐到期",
    "到期时间",
    "过期时间",
    "有效期至",
    "有效期到",
    "官网",
    "官方网站",
    "防失联",
    "remaining traffic",
    "traffic remaining",
    "traffic left",
    "bandwidth remaining",
    "bandwidth left",
    "next reset",
    "reset in",
    "expires at",
    "expire at",
    "expiry date",
    "expiration date",
    "valid until",
    "official website",
    "official site",
];

const LOOPBACK_INFO_NAME_MARKERS: &[&str] = &[
    "流量",
    "重置",
    "到期",
    "过期",
    "有效期",
    "套餐",
    "官网",
    "网站",
    "订阅",
    "公告",
    "通知",
    "提示",
    "客服",
    "联系",
    "更新",
    "失联",
    "域名",
    "请勿",
    "traffic",
    "bandwidth",
    "reset",
    "expire",
    "expiry",
    "expiration",
    "plan",
    "website",
    "official",
    "subscription",
    "notice",
    "support",
    "contact",
    "update",
    "telegram",
];

fn is_loopback_server(outbound: &serde_json::Value) -> bool {
    let Some(server) = outbound.get("server").and_then(|value| value.as_str()) else {
        return false;
    };
    let server = server.trim();
    if server.eq_ignore_ascii_case("localhost") {
        return true;
    }
    server
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// 机场常把流量、到期时间和官网等账户信息伪装成一个不可用代理节点。
/// 高置信名字直接过滤；较宽泛的名字只有在目标为 loopback 时才过滤，避免
/// 误伤名字里偶然含有 `traffic` / `套餐` 的真实远端节点。
pub(crate) fn is_informational_subscription_node(name: &str, outbound: &serde_json::Value) -> bool {
    let normalized_name = name.trim().to_lowercase();
    STRONG_INFO_NAME_MARKERS
        .iter()
        .any(|marker| normalized_name.contains(marker))
        || (is_loopback_server(outbound)
            && LOOPBACK_INFO_NAME_MARKERS
                .iter()
                .any(|marker| normalized_name.contains(marker)))
}

fn filter_informational_nodes(
    mut nodes: Vec<(String, serde_json::Value)>,
) -> (Vec<(String, serde_json::Value)>, usize) {
    let original_len = nodes.len();
    nodes.retain(|(name, outbound)| !is_informational_subscription_node(name, outbound));
    let filtered_count = original_len - nodes.len();
    (nodes, filtered_count)
}

async fn read_bounded_response_bytes(
    mut res: reqwest::Response,
    link: &str,
) -> Result<Vec<u8>, FetchError> {
    if res
        .content_length()
        .is_some_and(|length| length > MAX_SUBSCRIPTION_BYTES as u64)
    {
        return Err(oversized_subscription());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| {
        FetchError::request(
            format!("Failed to read subscription response from {}", link),
            e,
        )
    })? {
        if chunk.len() > MAX_SUBSCRIPTION_BYTES - bytes.len() {
            return Err(oversized_subscription());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn parse_subscription_body(text: &str, link: &str) -> Result<FetchResult, FetchError> {
    let parse_result = parse_clash_proxies(text).map_err(|e| {
        FetchError::parse(AppError::context(
            format!("Failed to parse subscription content from {}", link),
            e,
        ))
    })?;

    // A generic Clash config may omit proxies, but a subscription response
    // must explicitly contain a list. Maintenance text is not an empty pool.
    if !parse_result.has_proxy_list {
        return Err(FetchError::parse(AppError::message(
            "Subscription response does not contain a proxies list",
        )));
    }
    let total_count = parse_result.total_count;
    let parse_errors = parse_result.errors;
    let (nodes, filtered_info_count) = filter_informational_nodes(parse_result.nodes);
    let (node_names, outbounds): (Vec<String>, Vec<serde_json::Value>) = nodes.into_iter().unzip();

    // 解析错误将由调用方统一处理，此处不再打印
    Ok(FetchResult {
        node_names,
        outbounds,
        parse_errors,
        total_count,
        filtered_info_count,
    })
}

pub async fn fetch_sub(link: &str, client: &reqwest::Client) -> Result<FetchResult, FetchError> {
    let res = client
        .get(link)
        .timeout(std::time::Duration::from_secs(30))
        .header("User-Agent", "clash-meta")
        .send()
        .await
        .map_err(|e| FetchError::request(format!("Failed to fetch subscription from {}", link), e))?
        .error_for_status()
        .map_err(|e| {
            FetchError::request(
                format!("Subscription server returned HTTP error for {}", link),
                e,
            )
        })?;

    let bytes = read_bounded_response_bytes(res, link).await?;
    let text = String::from_utf8_lossy(&bytes);
    parse_subscription_body(&text, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscription_byte_limit_covers_chunked_and_fixed_length_bodies() {
        use axum::{
            body::{Body, Bytes},
            routing::get,
            Router,
        };

        for chunked in [false, true] {
            for extra in [0, 1] {
                // One real node followed by a comment: exactly 4 MiB must parse;
                // one byte more must fail, even without a Content-Length header.
                let mut yaml = "proxies:\n- {name: bounded, type: ss, server: example.com, port: 8388, cipher: aes-128-gcm, password: pass}\n#".to_string();
                yaml.push_str(&"x".repeat(4 * 1024 * 1024 + extra - yaml.len()));
                let data = Bytes::from(yaml);
                let app = Router::new().route(
                    "/sub",
                    get(move || {
                        let data = data.clone();
                        async move {
                            if chunked {
                                let chunks: Vec<_> = data
                                    .chunks(8192)
                                    .map(|chunk| {
                                        Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk))
                                    })
                                    .collect();
                                Body::from_stream(futures::stream::iter(chunks))
                            } else {
                                Body::from(data)
                            }
                        }
                    }),
                );
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();
                let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
                let result = fetch_sub(
                    &format!("http://{addr}/sub"),
                    &reqwest::Client::builder().no_proxy().build().unwrap(),
                )
                .await;
                server.abort();
                if extra == 0 {
                    let parsed = result.unwrap();
                    assert_eq!(parsed.node_names, vec!["bounded"]);
                    assert_eq!(parsed.outbounds[0]["server"], "example.com");
                } else {
                    let error = result.unwrap_err();
                    assert_eq!(error.kind, SubscriptionFailureKind::Parse);
                    assert!(error.to_string().contains("4 MiB"));
                }
            }
        }
    }

    #[test]
    fn subscription_node_limit_counts_entries_before_filtering() {
        let valid = "{name: kept, type: ss, server: example.com, port: 8388, cipher: aes-128-gcm, password: pass}";
        let yaml = format!("proxies: [{valid},{}]", vec!["null"; 9999].join(","));
        let result = parse_clash_proxies(&yaml).unwrap();
        assert_eq!(result.total_count, 10_000);
        assert_eq!(result.nodes.len(), 1);
        assert_eq!(result.nodes[0].0, "kept");
        let oversized = format!("proxies: [{valid},{}]", vec!["null"; 10_000].join(","));
        assert!(parse_clash_proxies(&oversized)
            .unwrap_err()
            .to_string()
            .contains("10000"));
    }

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
    fn informational_subscription_nodes_are_filtered_conservatively() {
        let yaml = r#"
proxies:
  - { name: "剩余流量：19.06 GB", type: ss, server: 127.0.0.1, port: 1, cipher: aes-128-gcm, password: info }
  - { name: "距离下次重置剩余：23 天", type: ss, server: 127.0.0.1, port: 2, cipher: aes-128-gcm, password: info }
  - { name: "套餐到期：2026-09-17", type: ss, server: 198.51.100.1, port: 3, cipher: aes-128-gcm, password: info }
  - { name: "官网 nachoneko.cc", type: ss, server: 127.0.0.1, port: 4, cipher: aes-128-gcm, password: info }
  - { name: "流量信息", type: ss, server: "::1", port: 5, cipher: aes-128-gcm, password: info }
  - { name: "本地开发节点", type: ss, server: 127.0.0.1, port: 8388, cipher: aes-128-gcm, password: pass }
  - { name: "套餐专线", type: ss, server: node.example.com, port: 8388, cipher: aes-128-gcm, password: pass }
"#;
        let parsed = parse_clash_proxies(yaml).unwrap();

        let (nodes, filtered_count) = filter_informational_nodes(parsed.nodes);

        assert_eq!(filtered_count, 5);
        assert_eq!(
            nodes
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["本地开发节点", "套餐专线"]
        );
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

    #[test]
    fn parse_subscription_body_distinguishes_missing_proxy_list_from_authoritative_empty_pool() {
        let maintenance = parse_subscription_body("port: 7890\nmode: rule\n", "https://sub.test")
            .expect_err("YAML without proxies list must be a parse failure");
        assert_eq!(maintenance.kind, SubscriptionFailureKind::Parse);

        let empty = parse_subscription_body("proxies: []\n", "https://sub.test")
            .expect("explicit empty list is authoritative");
        assert!(empty.outbounds.is_empty());
        assert_eq!(empty.total_count, 0);

        let info_only = parse_subscription_body(
            "proxies:\n  - { name: \"剩余流量：1 GB\", type: ss, server: 127.0.0.1, port: 1, cipher: aes-128-gcm, password: p }\n",
            "https://sub.test",
        )
        .expect("informational-only list is still an accepted empty pool");
        assert!(info_only.outbounds.is_empty());
        assert_eq!(info_only.filtered_info_count, 1);
    }
}
