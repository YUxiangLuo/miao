//! 地图模式:聚合当前活动连接并按目的 IP 做本地地理定位,供前端世界地图渲染。
//! 数据链路:Clash API(127.0.0.1:6262,仅监听本机)+ 内嵌 mmdb 数据库,全程不出本机。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use axum::{extract::State, Json};
use tracing::warn;

use crate::models::{ApiResponse, MapConnection, MapOverview, MapProxyPoint, MapSelfPoint};
use crate::services::config::runtime_outbound_server;
use crate::services::geoip::is_locatable;
use crate::state::{AppState, CachedLocation, CachedProxyLocation};

const CLASH_TIMEOUT: Duration = Duration::from_secs(5);
const ECHO_TIMEOUT: Duration = Duration::from_secs(8);
const LOCATION_CACHE_TTL: Duration = Duration::from_secs(600);
const MAX_MAP_CONNECTIONS: usize = 200;
/// IP 回显服务:探测本机真实出口。这两个域名已内置直连规则
/// (见 services::config::apply_route_mode),全局代理模式下同样走直连
const ECHO_URLS: [&str; 2] = ["https://cip.cc", "https://myip.ipip.net"];

static IPV4_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?:\d{1,3}\.){3}\d{1,3}").expect("valid IPv4 regex"));

pub async fn get_map_overview(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<MapOverview>> {
    // 服务停止时 Clash API 不可达:直接返回空数据,且不做任何外网探测。
    // 顺带让单测天然不触网——127.0.0.1:6262 拒绝连接后走这条短路路径。
    let Some(connections_payload) = fetch_clash_connections(&state).await else {
        return Json(ApiResponse::success(
            "sing-box is not running",
            MapOverview {
                running: false,
                self_point: None,
                proxy_point: None,
                connections: Vec::new(),
            },
        ));
    };

    let (self_point, proxy_point) = tokio::join!(
        resolve_self_location(&state),
        resolve_proxy_location(&state),
    );

    let connections = geolocate(&state, aggregate_connections(&connections_payload));

    Json(ApiResponse::success(
        "ok",
        MapOverview {
            running: true,
            self_point,
            proxy_point,
            connections,
        },
    ))
}

async fn fetch_clash_connections(state: &Arc<AppState>) -> Option<serde_json::Value> {
    state
        .http_client
        .get(format!("{}/connections", state.clash_api_base))
        .timeout(CLASH_TIMEOUT)
        .send()
        .await
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()
}

/// 按 (目的 IP, 协议, 出口) 聚合连接,字节数求和;按总流量降序截取前 N 条。
/// 面板展示与地图渲染都不需要逐连接粒度。
#[derive(Debug, PartialEq)]
pub(crate) struct AggregatedConnection {
    pub ip: String,
    pub host: Option<String>,
    pub network: String,
    pub proxied: bool,
    pub up: u64,
    pub down: u64,
}

pub(crate) fn aggregate_connections(payload: &serde_json::Value) -> Vec<AggregatedConnection> {
    let mut aggregated: HashMap<(String, String, bool), AggregatedConnection> = HashMap::new();
    let Some(connections) = payload["connections"].as_array() else {
        return Vec::new();
    };
    for conn in connections {
        let metadata = &conn["metadata"];
        let Some(ip) = metadata["destinationIP"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        let network = metadata["network"].as_str().unwrap_or("tcp").to_string();
        let host = metadata["host"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let proxied = !conn["chains"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|chain| chain.as_str())
            .any(|chain| chain == "direct");
        let up = conn["upload"].as_u64().unwrap_or(0);
        let down = conn["download"].as_u64().unwrap_or(0);

        let entry = aggregated
            .entry((ip.to_string(), network.clone(), proxied))
            .or_insert_with(|| AggregatedConnection {
                ip: ip.to_string(),
                network,
                host: host.clone(),
                proxied,
                up: 0,
                down: 0,
            });
        entry.up += up;
        entry.down += down;
        if entry.host.is_none() && host.is_some() {
            entry.host = host;
        }
    }
    let mut list: Vec<_> = aggregated.into_values().collect();
    list.sort_by_key(|conn| std::cmp::Reverse(conn.up + conn.down));
    list.truncate(MAX_MAP_CONNECTIONS);
    list
}

fn geolocate(state: &AppState, aggregated: Vec<AggregatedConnection>) -> Vec<MapConnection> {
    aggregated
        .into_iter()
        .filter_map(|conn| {
            state.geo.lookup_str(&conn.ip).map(|point| MapConnection {
                ip: conn.ip,
                host: conn.host,
                network: conn.network,
                lat: point.lat,
                lng: point.lng,
                country: point.country,
                city: point.city,
                up: conn.up,
                down: conn.down,
                proxied: conn.proxied,
            })
        })
        .collect()
}

/// 解析 config.yaml 中 `location: "lat,lng"` 手动覆盖
fn parse_location_override(raw: &str) -> Option<(f64, f64)> {
    let mut parts = raw.trim().split(',');
    let lat = parts.next()?.trim().parse::<f64>().ok()?;
    let lng = parts.next()?.trim().parse::<f64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return None;
    }
    Some((lat, lng))
}

async fn resolve_self_location(state: &Arc<AppState>) -> Option<MapSelfPoint> {
    let override_location = {
        let config = state.config.read().await;
        config.location.as_deref().and_then(parse_location_override)
    };
    if let Some((lat, lng)) = override_location {
        return Some(MapSelfPoint {
            ip: None,
            lat,
            lng,
            country: None,
            city: None,
        });
    }

    {
        let cache = state.map_cache.lock().await;
        if let Some(entry) = &cache.self_location {
            if entry.fetched_at.elapsed() < LOCATION_CACHE_TTL {
                return entry.point.clone().map(|point| MapSelfPoint {
                    ip: Some(entry.ip.clone()),
                    lat: point.lat,
                    lng: point.lng,
                    country: point.country,
                    city: point.city,
                });
            }
        }
    }

    let ip = fetch_public_ip(state).await?;
    let point = state.geo.lookup_str(&ip);
    // 即使定位失败也缓存探测到的 IP,避免每个轮询周期都打回显服务
    state.map_cache.lock().await.self_location = Some(CachedLocation {
        fetched_at: Instant::now(),
        ip: ip.clone(),
        point: point.clone(),
    });
    point.map(|point| MapSelfPoint {
        ip: Some(ip),
        lat: point.lat,
        lng: point.lng,
        country: point.country,
        city: point.city,
    })
}

async fn fetch_public_ip(state: &Arc<AppState>) -> Option<String> {
    for url in ECHO_URLS {
        let Ok(response) = state
            .http_client
            .get(url)
            .timeout(ECHO_TIMEOUT)
            .send()
            .await
        else {
            continue;
        };
        let Ok(text) = response.text().await else {
            continue;
        };
        if let Some(ip) = extract_first_ipv4(&text) {
            return Some(ip);
        }
        warn!(
            url = url,
            "Echo service response did not contain a valid IPv4 address"
        );
    }
    None
}

fn extract_first_ipv4(text: &str) -> Option<String> {
    IPV4_RE
        .find(text)
        .and_then(|m| m.as_str().parse::<IpAddr>().ok())
        .map(|ip| ip.to_string())
}

async fn resolve_proxy_location(state: &Arc<AppState>) -> Option<MapProxyPoint> {
    let node = fetch_selected_node(state).await?;

    {
        let cache = state.map_cache.lock().await;
        if let Some(entry) = &cache.proxy_location {
            if entry.node == node && entry.fetched_at.elapsed() < LOCATION_CACHE_TTL {
                return entry.point.clone().map(|point| MapProxyPoint {
                    node: node.clone(),
                    ip: entry.ip.clone(),
                    lat: point.lat,
                    lng: point.lng,
                    country: point.country,
                    city: point.city,
                });
            }
        }
    }

    let server = runtime_outbound_server(&node).await;
    let resolved_ip = match &server {
        Some(server) => resolve_node_server(state, server).await,
        None => None,
    };
    let point = resolved_ip.and_then(|ip| state.geo.lookup(ip));
    let display_ip = resolved_ip.map(|ip| ip.to_string()).or(server.clone());

    state.map_cache.lock().await.proxy_location = Some(CachedProxyLocation {
        node: node.clone(),
        fetched_at: Instant::now(),
        ip: display_ip.clone(),
        point: point.clone(),
    });

    point.map(|point| MapProxyPoint {
        node,
        ip: display_ip,
        lat: point.lat,
        lng: point.lng,
        country: point.country,
        city: point.city,
    })
}

async fn fetch_selected_node(state: &Arc<AppState>) -> Option<String> {
    let payload: serde_json::Value = state
        .http_client
        .get(format!("{}/proxies", state.clash_api_base))
        .timeout(CLASH_TIMEOUT)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    payload["proxies"]["proxy"]["now"]
        .as_str()
        .map(str::to_string)
}

/// 解析节点服务器地址为可定位的公网 IP。
/// 机场入口域名常对海外解析器返回占位 IP(如 127.127.127.5),
/// 本地解析(经 sing-box DNS)拿不到可定位结果时,回退到国内 DoH 直连查询。
async fn resolve_node_server(state: &Arc<AppState>, server: &str) -> Option<IpAddr> {
    if let Ok(ip) = server.parse::<IpAddr>() {
        return is_locatable(ip).then_some(ip);
    }
    // 常规域名:本地解析最快,且大概率已是真实入口
    if let Some(ip) = tokio::net::lookup_host((server, 443))
        .await
        .ok()
        .and_then(|mut addrs| addrs.find(|addr| is_locatable(addr.ip())))
        .map(|addr| addr.ip())
    {
        return Some(ip);
    }
    resolve_via_doh(state, server).await
}

/// DoH JSON API 回退解析;AliDNS 优先(国内解析器才能拿到机场的真实入口 IP)
async fn resolve_via_doh(state: &Arc<AppState>, server: &str) -> Option<IpAddr> {
    let encoded = urlencoding::encode(server);
    let urls = [
        format!("https://dns.alidns.com/resolve?name={encoded}&type=A"),
        format!("https://1.1.1.1/dns-query?name={encoded}&type=A"),
    ];
    for url in urls {
        let request = state
            .http_client
            .get(&url)
            .header("accept", "application/dns-json")
            .timeout(CLASH_TIMEOUT);
        let Ok(response) = request.send().await else {
            continue;
        };
        let Ok(payload) = response.json::<serde_json::Value>().await else {
            continue;
        };
        if let Some(ip) = parse_doh_answers(&payload) {
            return Some(ip);
        }
    }
    None
}

/// 从 DoH JSON 响应中取第一个可定位的 A/AAAA 记录
fn parse_doh_answers(payload: &serde_json::Value) -> Option<IpAddr> {
    payload["Answer"]
        .as_array()?
        .iter()
        .filter(|answer| answer["type"].as_u64() == Some(1) || answer["type"].as_u64() == Some(28))
        .filter_map(|answer| answer["data"].as_str()?.parse::<IpAddr>().ok())
        .find(|ip| is_locatable(*ip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Config;
    use crate::test_support::{app_state, empty_request, response_json};
    use serde_json::json;
    use tower::ServiceExt;

    fn sample_connections_payload() -> serde_json::Value {
        json!({
            "downloadTotal": 0,
            "uploadTotal": 0,
            "connections": [
                {
                    "id": "1",
                    "upload": 100,
                    "download": 1000,
                    "chains": ["direct"],
                    "metadata": {"network": "tcp", "destinationIP": "223.5.5.5", "host": "a.cn", "destinationPort": 443}
                },
                {
                    "id": "2",
                    "upload": 50,
                    "download": 500,
                    "chains": ["direct"],
                    "metadata": {"network": "tcp", "destinationIP": "223.5.5.5", "host": "b.cn", "destinationPort": 443}
                },
                {
                    "id": "3",
                    "upload": 7,
                    "download": 70,
                    "chains": ["HK-Node", "proxy"],
                    "metadata": {"network": "udp", "destinationIP": "8.8.8.8", "host": "dns.google", "destinationPort": 443}
                },
                {
                    "id": "4",
                    "upload": 1,
                    "download": 1,
                    "chains": ["direct"],
                    "metadata": {"network": "tcp", "destinationIP": "", "host": "only-host.cn", "destinationPort": 443}
                }
            ]
        })
    }

    #[test]
    fn aggregate_connections_merges_same_ip_network_and_chain() {
        let result = aggregate_connections(&sample_connections_payload());
        let direct = result
            .iter()
            .find(|c| c.ip == "223.5.5.5")
            .expect("direct entry exists");
        assert_eq!(direct.up, 150);
        assert_eq!(direct.down, 1500);
        assert!(!direct.proxied);
        assert_eq!(direct.host.as_deref(), Some("a.cn"));
    }

    #[test]
    fn aggregate_connections_marks_proxy_chain_as_proxied() {
        let result = aggregate_connections(&sample_connections_payload());
        let proxied = result
            .iter()
            .find(|c| c.ip == "8.8.8.8")
            .expect("proxied entry exists");
        assert!(proxied.proxied);
        assert_eq!(proxied.network, "udp");
    }

    #[test]
    fn aggregate_connections_skips_entries_without_destination_ip() {
        let result = aggregate_connections(&sample_connections_payload());
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn aggregate_connections_handles_missing_connections_field() {
        assert!(aggregate_connections(&json!({})).is_empty());
    }

    #[test]
    fn aggregate_connections_sorts_by_total_bytes_desc() {
        let result = aggregate_connections(&sample_connections_payload());
        assert_eq!(result[0].ip, "223.5.5.5");
        assert_eq!(result[1].ip, "8.8.8.8");
    }

    #[test]
    fn aggregate_connections_caps_at_max_entries() {
        let connections: Vec<_> = (0..250)
            .map(|i| {
                json!({
                    "id": i.to_string(),
                    "upload": i,
                    "download": i,
                    "chains": ["direct"],
                    "metadata": {"network": "tcp", "destinationIP": format!("10.0.{}.{}", i / 256, i % 256)}
                })
            })
            .collect();
        let payload = json!({ "connections": connections });
        assert_eq!(aggregate_connections(&payload).len(), MAX_MAP_CONNECTIONS);
    }

    #[test]
    fn parse_location_override_accepts_valid_coordinates() {
        assert_eq!(
            parse_location_override("31.23, 121.47"),
            Some((31.23, 121.47))
        );
    }

    #[test]
    fn parse_location_override_rejects_invalid_input() {
        assert_eq!(parse_location_override(""), None);
        assert_eq!(parse_location_override("31.23"), None);
        assert_eq!(parse_location_override("31.23,121.47,extra"), None);
        assert_eq!(parse_location_override("91,0"), None);
        assert_eq!(parse_location_override("abc,def"), None);
    }

    #[test]
    fn extract_first_ipv4_finds_valid_ip() {
        assert_eq!(
            extract_first_ipv4("当前 IP：203.0.113.10 来自于：中国"),
            Some("203.0.113.10".to_string())
        );
        assert_eq!(extract_first_ipv4("no ip here"), None);
        assert_eq!(extract_first_ipv4("999.999.999.999"), None);
    }

    #[test]
    fn self_location_override_short_circuits_network_probe() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let state = app_state(Config {
                location: Some("31.23,121.47".to_string()),
                ..Default::default()
            });
            let point = resolve_self_location(&state).await.expect("override point");
            assert_eq!((point.lat, point.lng), (31.23, 121.47));
            assert!(point.ip.is_none());
        });
    }

    #[test]
    fn parse_doh_answers_picks_first_locatable_record() {
        let payload = json!({
            "Answer": [
                {"type": 5, "data": "alias.example.com"},
                {"type": 1, "data": "127.127.127.5"},
                {"type": 1, "data": "36.141.40.13"}
            ]
        });
        assert_eq!(
            parse_doh_answers(&payload),
            Some("36.141.40.13".parse().unwrap())
        );
        assert_eq!(parse_doh_answers(&json!({})), None);
        assert_eq!(parse_doh_answers(&json!({"Answer": []})), None);
    }

    #[tokio::test]
    async fn map_overview_returns_not_running_when_clash_api_unreachable() {
        // 控制面指向不可达地址(开发机上 systemd 常驻服务占着真实的 6262)
        let config_path = std::env::temp_dir().join(format!(
            "miao-test-map-overview-{}.yaml",
            std::process::id()
        ));
        let state = Arc::new(
            AppState::with_config_path(Config::default(), config_path)
                .unwrap()
                .with_clash_api_base("http://127.0.0.1:1"),
        );
        state
            .initializing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let app = crate::router::build_router(state);
        let response = app
            .oneshot(empty_request("GET", "/api/map/overview"))
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["success"], json!(true));
        assert_eq!(body["data"]["running"], json!(false));
        assert_eq!(body["data"]["connections"], json!([]));
    }
}
