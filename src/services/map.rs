use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tracing::warn;

use crate::error::{AppError, AppResult};
use crate::models::{
    ClientEntity, DestinationEntity, GeoLocation, MapSnapshot, NetworkFlow, ProxyEntity,
};
use crate::services::geo::{
    cache_path_for_config, destination_geo_query, load_cache, now_unix, proxy_geo_query,
    resolve_queries, SELF_QUERY,
};
use crate::services::node_geo::geo_from_node_name;
use crate::services::node_parser::parse_node_json;
use crate::services::singbox::get_sing_box_home;
use crate::state::{AppState, ConnectionByteSample};

const CLASH_CONNECTIONS_URL: &str = "http://127.0.0.1:6262/connections";
const SKIP_OUTBOUND_TYPES: &[&str] = &["selector", "urltest", "direct", "block", "dns"];

#[derive(Debug, Clone)]
pub struct ClashConnection {
    pub id: String,
    pub network: String,
    pub host: Option<String>,
    pub destination_ip: String,
    pub destination_port: Option<u16>,
    pub upload: u64,
    pub download: u64,
    pub chains: Vec<String>,
    pub rule: Option<String>,
}

pub fn parse_clash_connection(value: &Value) -> Option<ClashConnection> {
    let id = value.get("id")?.as_str()?.to_string();
    let metadata = value.get("metadata").cloned().unwrap_or(Value::Null);
    let network = metadata
        .get("network")
        .and_then(Value::as_str)
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let host = first_non_empty_str(&metadata, &["host", "sniffHost", "remoteDestination"]);
    let destination_ip = first_non_empty_str(
        &metadata,
        &["destinationIP", "destination_ip", "destination"],
    )
    .unwrap_or_default();
    let destination_port = metadata
        .get("destinationPort")
        .or_else(|| metadata.get("destination_port"))
        .and_then(parse_port);
    let upload = value.get("upload").and_then(Value::as_u64).unwrap_or(0);
    let download = value.get("download").and_then(Value::as_u64).unwrap_or(0);
    let chains = value
        .get("chains")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let rule = {
        let name = value.get("rule").and_then(Value::as_str).unwrap_or("");
        let payload = value
            .get("rulePayload")
            .and_then(Value::as_str)
            .unwrap_or("");
        if name.is_empty() && payload.is_empty() {
            None
        } else if payload.is_empty() {
            Some(name.to_string())
        } else if name.is_empty() {
            Some(payload.to_string())
        } else {
            Some(format!("{name} : {payload}"))
        }
    };

    Some(ClashConnection {
        id,
        network: if network == "udp" {
            "udp".into()
        } else {
            "tcp".into()
        },
        host,
        destination_ip,
        destination_port,
        upload,
        download,
        chains,
        rule,
    })
}

fn first_non_empty_str(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string)
    })
}

fn parse_port(value: &Value) -> Option<u16> {
    if let Some(number) = value.as_u64() {
        return u16::try_from(number).ok().filter(|port| *port > 0);
    }
    value
        .as_str()
        .and_then(|text| text.parse::<u16>().ok())
        .filter(|port| *port > 0)
}

pub fn leaf_outbound(chains: &[String]) -> Option<&str> {
    chains
        .iter()
        .map(String::as_str)
        .find(|name| !name.eq_ignore_ascii_case("proxy"))
}

pub fn is_direct_chain(chains: &[String]) -> bool {
    match leaf_outbound(chains) {
        Some(name) => name.eq_ignore_ascii_case("direct"),
        None => false,
    }
}

fn unresolved_proxy(name: &str) -> ProxyEntity {
    ProxyEntity {
        entity_type: "proxy",
        name: name.to_string(),
        server: String::new(),
        geo: geo_from_node_name(name),
    }
}

pub fn extract_proxy_servers(config_json: &Value) -> Vec<(String, String)> {
    let Some(outbounds) = config_json.get("outbounds").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut servers = Vec::new();
    for outbound in outbounds {
        let outbound_type = outbound
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if SKIP_OUTBOUND_TYPES
            .iter()
            .any(|skipped| outbound_type.eq_ignore_ascii_case(skipped))
        {
            continue;
        }
        let Some(tag) = outbound.get("tag").and_then(Value::as_str) else {
            continue;
        };
        let Some(server) = outbound
            .get("server")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if seen.insert(tag.to_string()) {
            servers.push((tag.to_string(), server.to_string()));
        }
    }
    servers
}

pub fn proxies_from_manual_nodes(nodes: &[String]) -> Vec<(String, String)> {
    let mut servers = Vec::new();
    for node in nodes {
        if let Ok((info, _)) = parse_node_json(node) {
            servers.push((info.tag, info.server));
        }
    }
    servers
}

pub fn client_display_name() -> &'static str {
    if Path::new("/etc/openwrt_release").exists() {
        "Router"
    } else {
        "This Device"
    }
}

fn domain_from_host(host: Option<&str>, ip: &str) -> Option<String> {
    let host = host.map(str::trim).filter(|value| !value.is_empty())?;
    if host == ip || host.parse::<std::net::IpAddr>().is_ok() {
        None
    } else {
        Some(host.to_string())
    }
}

pub fn compute_speeds(
    samples: &mut HashMap<String, ConnectionByteSample>,
    connections: &[ClashConnection],
    now: Instant,
) -> HashMap<String, (f64, f64)> {
    let mut speeds = HashMap::new();
    let mut seen = HashSet::new();

    for connection in connections {
        seen.insert(connection.id.clone());
        let (upload_speed, download_speed) = match samples.get(&connection.id) {
            Some(previous) => {
                let elapsed = now.saturating_duration_since(previous.at).as_secs_f64();
                if elapsed <= 0.0 {
                    (0.0, 0.0)
                } else {
                    (
                        (connection.upload.saturating_sub(previous.upload) as f64 / elapsed)
                            .max(0.0),
                        (connection.download.saturating_sub(previous.download) as f64 / elapsed)
                            .max(0.0),
                    )
                }
            }
            None => (0.0, 0.0),
        };
        speeds.insert(connection.id.clone(), (upload_speed, download_speed));
        samples.insert(
            connection.id.clone(),
            ConnectionByteSample {
                upload: connection.upload,
                download: connection.download,
                at: now,
            },
        );
    }

    samples.retain(|id, _| seen.contains(id));
    speeds
}

fn lookup_geo(
    resolved: &HashMap<String, Option<GeoLocation>>,
    query: Option<&str>,
) -> Option<GeoLocation> {
    query.and_then(|key| resolved.get(key).cloned().flatten())
}

/// sing-box 运行中（TUN 接管）时，self 查询会经代理出口，只能信任
/// 启动前直连环境下写入的缓存；未运行时走实时解析结果。
pub fn resolve_client_geo(
    running: bool,
    cache: &crate::services::geo::GeoCache,
    resolved: &HashMap<String, Option<GeoLocation>>,
    now: u64,
) -> Option<GeoLocation> {
    if running {
        cache.get(SELF_QUERY, now).flatten()
    } else {
        resolved.get(SELF_QUERY).cloned().flatten()
    }
}

pub fn build_snapshot(
    client_name: &str,
    client_geo: Option<GeoLocation>,
    proxy_servers: &[(String, String)],
    connections: &[ClashConnection],
    speeds: &HashMap<String, (f64, f64)>,
    resolved: &HashMap<String, Option<GeoLocation>>,
) -> MapSnapshot {
    let proxies: Vec<ProxyEntity> = proxy_servers
        .iter()
        .map(|(name, server)| ProxyEntity {
            entity_type: "proxy",
            name: name.clone(),
            server: server.clone(),
            // 节点名（国旗/地区关键字）比中转入口域名的 GeoIP 更能代表出口位置
            geo: geo_from_node_name(name)
                .or_else(|| lookup_geo(resolved, proxy_geo_query(server).as_deref())),
        })
        .collect();
    let proxy_by_name: HashMap<&str, &ProxyEntity> = proxies
        .iter()
        .map(|proxy| (proxy.name.as_str(), proxy))
        .collect();

    let flows = connections
        .iter()
        .map(|connection| {
            let dest_query =
                destination_geo_query(&connection.destination_ip, connection.host.as_deref());
            let destination = DestinationEntity {
                entity_type: "destination",
                domain: domain_from_host(connection.host.as_deref(), &connection.destination_ip),
                ip: connection.destination_ip.clone(),
                geo: lookup_geo(resolved, dest_query.as_deref()),
            };
            let proxy = if is_direct_chain(&connection.chains) {
                None
            } else {
                Some(match leaf_outbound(&connection.chains) {
                    Some(name) => proxy_by_name
                        .get(name)
                        .cloned()
                        .cloned()
                        .unwrap_or_else(|| unresolved_proxy(name)),
                    None => unresolved_proxy("proxy"),
                })
            };
            let (upload_speed, download_speed) =
                speeds.get(&connection.id).copied().unwrap_or((0.0, 0.0));
            NetworkFlow {
                id: connection.id.clone(),
                destination,
                proxy,
                network: connection.network.clone(),
                upload_speed,
                download_speed,
                upload_total: connection.upload,
                download_total: connection.download,
                rule: connection.rule.clone(),
                port: connection.destination_port,
            }
        })
        .collect();

    MapSnapshot {
        client: ClientEntity {
            entity_type: "client",
            name: client_name.to_string(),
            geo: client_geo,
        },
        proxies,
        flows,
    }
}

async fn read_runtime_outbounds() -> Vec<(String, String)> {
    let path = get_sing_box_home().join("config.json");
    let Ok(text) = tokio::fs::read_to_string(path).await else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    extract_proxy_servers(&value)
}

async fn fetch_clash_connections(state: &Arc<AppState>) -> AppResult<Vec<ClashConnection>> {
    let response = state
        .http_client
        .get(CLASH_CONNECTIONS_URL)
        .send()
        .await
        .map_err(|e| AppError::context("Failed to read Clash connections", e))?;
    if !response.status().is_success() {
        return Err(AppError::message(format!(
            "Clash connections API returned {}",
            response.status()
        )));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|e| AppError::context("Failed to parse Clash connections", e))?;
    let connections = payload
        .get("connections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(connections
        .iter()
        .filter_map(parse_clash_connection)
        .collect())
}

async fn sing_box_is_running(state: &Arc<AppState>) -> bool {
    let mut lock = state.sing_process.lock().await;
    match &mut *lock {
        Some(proc) => match proc.child.try_wait() {
            Ok(Some(_)) => {
                *lock = None;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        },
        None => false,
    }
}

pub async fn collect_map_snapshot(state: &Arc<AppState>) -> MapSnapshot {
    let running = sing_box_is_running(state).await;
    let manual_nodes = {
        let config = state.config.read().await;
        proxies_from_manual_nodes(&config.nodes)
    };

    let mut proxy_servers = if running {
        let mut runtime = read_runtime_outbounds().await;
        if runtime.is_empty() {
            runtime = manual_nodes;
        }
        runtime
    } else {
        manual_nodes
    };

    let connections = if running {
        match fetch_clash_connections(state).await {
            Ok(connections) => connections,
            Err(error) => {
                warn!(error = %error, "Map snapshot could not read Clash connections");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };

    let speeds = {
        let mut samples = state.connection_bytes.lock().await;
        compute_speeds(&mut samples, &connections, Instant::now())
    };

    let mut queries = Vec::new();
    if !running {
        // TUN 未接管时才实时查询本机定位；运行中只能用启动前缓存的值
        queries.push(SELF_QUERY.to_string());
    }
    for (_, server) in &proxy_servers {
        if let Some(query) = proxy_geo_query(server) {
            queries.push(query);
        }
    }
    for connection in &connections {
        if let Some(query) =
            destination_geo_query(&connection.destination_ip, connection.host.as_deref())
        {
            queries.push(query);
        }
    }
    queries.sort();
    queries.dedup();

    let cache_path = cache_path_for_config(&state.config_path);
    let mut local_cache = {
        let mut cache = state.geo_cache.lock().await;
        if !cache.loaded {
            *cache = load_cache(&cache_path).await;
        }
        cache.clone()
    };
    let resolved =
        resolve_queries(&state.http_client, &mut local_cache, &queries, &cache_path).await;
    {
        let mut cache = state.geo_cache.lock().await;
        cache.merge_newer(&local_cache);
    }

    let client_geo = resolve_client_geo(running, &local_cache, &resolved, now_unix());
    // Keep proxy list stable even if runtime + manual overlap after a refresh.
    let mut seen = HashSet::new();
    proxy_servers.retain(|(name, _)| seen.insert(name.clone()));

    build_snapshot(
        client_display_name(),
        client_geo,
        &proxy_servers,
        &connections,
        &speeds,
        &resolved,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build_snapshot, compute_speeds, extract_proxy_servers, is_direct_chain, leaf_outbound,
        parse_clash_connection, resolve_client_geo, ClashConnection,
    };
    use crate::models::GeoLocation;
    use crate::services::geo::{GeoCache, SELF_QUERY};
    use crate::state::ConnectionByteSample;
    use serde_json::json;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    fn tokyo_geo() -> GeoLocation {
        GeoLocation {
            country: Some("Japan".into()),
            country_code: Some("JP".into()),
            city: Some("Tokyo".into()),
            latitude: Some(35.6),
            longitude: Some(139.7),
        }
    }

    fn frankfurt_geo() -> GeoLocation {
        GeoLocation {
            country: Some("Germany".into()),
            country_code: Some("DE".into()),
            city: Some("Frankfurt".into()),
            latitude: Some(50.1),
            longitude: Some(8.6),
        }
    }

    #[test]
    fn parse_clash_connection_reads_metadata_and_rule() {
        let value = json!({
            "id": "abc",
            "metadata": {
                "network": "UDP",
                "host": "googlevideo.com",
                "destinationIP": "142.250.1.1",
                "destinationPort": "443"
            },
            "upload": 10,
            "download": 20,
            "chains": ["Tokyo 01", "proxy"],
            "rule": "geosite",
            "rulePayload": "youtube"
        });

        let connection = parse_clash_connection(&value).unwrap();
        assert_eq!(connection.network, "udp");
        assert_eq!(connection.host.as_deref(), Some("googlevideo.com"));
        assert_eq!(connection.destination_ip, "142.250.1.1");
        assert_eq!(connection.destination_port, Some(443));
        assert_eq!(connection.rule.as_deref(), Some("geosite : youtube"));
        assert!(!is_direct_chain(&connection.chains));
        assert_eq!(leaf_outbound(&connection.chains), Some("Tokyo 01"));
    }

    #[test]
    fn direct_chain_treats_direct_leaf_as_direct() {
        assert!(is_direct_chain(&["direct".into(), "proxy".into()]));
        assert!(!is_direct_chain(&[]));
        assert!(!is_direct_chain(&["proxy".into()]));
        assert!(!is_direct_chain(&["Tokyo 01".into()]));
    }

    #[test]
    fn extract_proxy_servers_skips_built_in_outbounds() {
        let config = json!({
            "outbounds": [
                {"type": "selector", "tag": "proxy", "outbounds": ["Tokyo 01"]},
                {"type": "direct", "tag": "direct"},
                {"type": "hysteria2", "tag": "Tokyo 01", "server": "tokyo.example.com"},
                {"type": "shadowsocks", "tag": "Home", "server": ""}
            ]
        });

        assert_eq!(
            extract_proxy_servers(&config),
            vec![("Tokyo 01".into(), "tokyo.example.com".into())]
        );
    }

    #[test]
    fn compute_speeds_uses_byte_deltas() {
        let now = Instant::now();
        let earlier = now - Duration::from_secs(2);
        let mut samples = HashMap::new();
        samples.insert(
            "abc".into(),
            ConnectionByteSample {
                upload: 100,
                download: 500,
                at: earlier,
            },
        );
        let connections = vec![ClashConnection {
            id: "abc".into(),
            network: "tcp".into(),
            host: None,
            destination_ip: "1.1.1.1".into(),
            destination_port: Some(443),
            upload: 300,
            download: 2500,
            chains: vec!["direct".into()],
            rule: None,
        }];

        let speeds = compute_speeds(&mut samples, &connections, now);
        let (up, down) = speeds.get("abc").copied().unwrap();
        assert!((up - 100.0).abs() < 0.01);
        assert!((down - 1000.0).abs() < 0.01);
    }

    #[test]
    fn build_snapshot_maps_direct_and_proxy_paths() {
        let connections = vec![
            ClashConnection {
                id: "yt".into(),
                network: "tcp".into(),
                host: Some("youtube.com".into()),
                destination_ip: "142.250.1.1".into(),
                destination_port: Some(443),
                upload: 1,
                download: 8,
                chains: vec!["Node A".into(), "proxy".into()],
                rule: Some("final".into()),
            },
            ClashConnection {
                id: "gh".into(),
                network: "tcp".into(),
                host: Some("github.com".into()),
                destination_ip: "20.1.1.1".into(),
                destination_port: Some(443),
                upload: 2,
                download: 4,
                chains: vec!["direct".into()],
                rule: Some("geosite".into()),
            },
        ];
        let mut speeds = HashMap::new();
        speeds.insert("yt".into(), (10.0, 40.0));
        speeds.insert("gh".into(), (1.0, 2.0));
        let mut resolved = HashMap::new();
        resolved.insert("tokyo.example.com".into(), Some(tokyo_geo()));
        resolved.insert("142.250.1.1".into(), Some(frankfurt_geo()));

        // 节点名不含地区关键字时，走服务器地址的 GeoIP 结果
        let snapshot = build_snapshot(
            "This Device",
            None,
            &[("Node A".into(), "tokyo.example.com".into())],
            &connections,
            &speeds,
            &resolved,
        );

        assert_eq!(snapshot.proxies.len(), 1);
        assert_eq!(
            snapshot.proxies[0].geo.as_ref().unwrap().city.as_deref(),
            Some("Tokyo")
        );
        assert_eq!(snapshot.flows.len(), 2);
        let youtube = snapshot.flows.iter().find(|flow| flow.id == "yt").unwrap();
        assert_eq!(youtube.destination.domain.as_deref(), Some("youtube.com"));
        assert_eq!(
            youtube.proxy.as_ref().map(|proxy| proxy.name.as_str()),
            Some("Node A")
        );
        assert_eq!(
            youtube.destination.geo.as_ref().unwrap().city.as_deref(),
            Some("Frankfurt")
        );
        let github = snapshot.flows.iter().find(|flow| flow.id == "gh").unwrap();
        assert!(github.proxy.is_none());
        assert_eq!(github.download_speed, 2.0);
    }

    #[test]
    fn resolve_client_geo_prefers_cache_while_running() {
        let mut cache = GeoCache::default();
        cache.insert(SELF_QUERY.to_string(), Some(tokyo_geo()), 1_000);
        let mut resolved = HashMap::new();
        resolved.insert(SELF_QUERY.to_string(), Some(frankfurt_geo()));

        // 运行中：TUN 已接管，实时解析不可信，只用启动前缓存
        let geo = resolve_client_geo(true, &cache, &resolved, 1_500).unwrap();
        assert_eq!(geo.city.as_deref(), Some("Tokyo"));

        // 未运行：直连环境，用实时解析结果
        let geo = resolve_client_geo(false, &cache, &resolved, 1_500).unwrap();
        assert_eq!(geo.city.as_deref(), Some("Frankfurt"));
    }

    #[test]
    fn resolve_client_geo_running_with_expired_cache_is_none() {
        let mut cache = GeoCache::default();
        cache.insert(SELF_QUERY.to_string(), Some(tokyo_geo()), 1_000);
        let resolved = HashMap::new();

        let expired = 1_000 + 8 * 24 * 3600;
        assert!(resolve_client_geo(true, &cache, &resolved, expired).is_none());
        assert!(resolve_client_geo(true, &cache, &resolved, 1_500).is_some());
    }

    #[test]
    fn build_snapshot_uses_name_geo_when_server_is_unresolvable() {
        let snapshot = build_snapshot(
            "This Device",
            None,
            &[("🇭🇰 香港W01".into(), "relay.entry.invalid".into())],
            &[],
            &HashMap::new(),
            &HashMap::new(),
        );

        let geo = snapshot.proxies[0].geo.as_ref().unwrap();
        assert_eq!(geo.country_code.as_deref(), Some("HK"));
        assert!(geo.latitude.is_some());
    }

    #[test]
    fn selector_only_chain_is_treated_as_proxy() {
        let connections = vec![ClashConnection {
            id: "sel".into(),
            network: "tcp".into(),
            host: Some("example.com".into()),
            destination_ip: "1.2.3.4".into(),
            destination_port: Some(443),
            upload: 0,
            download: 0,
            chains: vec!["proxy".into()],
            rule: None,
        }];

        let snapshot = build_snapshot(
            "This Device",
            None,
            &[],
            &connections,
            &HashMap::new(),
            &HashMap::new(),
        );

        assert_eq!(
            snapshot.flows[0]
                .proxy
                .as_ref()
                .map(|proxy| proxy.name.as_str()),
            Some("proxy")
        );
    }
}
