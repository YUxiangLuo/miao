use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::error::{AppError, AppResult};
use crate::models::GeoLocation;

const SUCCESS_TTL_SECS: u64 = 7 * 24 * 3600;
const MISS_TTL_SECS: u64 = 6 * 3600;
const FAILURE_TTL_SECS: u64 = 3 * 60;
const MAX_CACHE_ENTRIES: usize = 4000;
const BATCH_SIZE: usize = 100;
const IP_API_FIELDS: &str = "status,message,country,countryCode,city,lat,lon,query";
pub const SELF_QUERY: &str = "self";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeoCacheEntry {
    pub geo: Option<GeoLocation>,
    pub fetched_at: u64,
    #[serde(default)]
    pub failure: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GeoCache {
    #[serde(default)]
    pub entries: HashMap<String, GeoCacheEntry>,
    #[serde(skip)]
    pub loaded: bool,
}

impl GeoCache {
    pub fn get(&self, query: &str, now: u64) -> Option<Option<GeoLocation>> {
        let entry = self.entries.get(query)?;
        let ttl = if entry.geo.is_some() {
            SUCCESS_TTL_SECS
        } else if entry.failure {
            FAILURE_TTL_SECS
        } else {
            MISS_TTL_SECS
        };
        if now.saturating_sub(entry.fetched_at) > ttl {
            return None;
        }
        Some(entry.geo.clone())
    }

    pub fn insert(&mut self, query: String, geo: Option<GeoLocation>, now: u64) {
        self.store(
            query,
            GeoCacheEntry {
                geo,
                fetched_at: now,
                failure: false,
            },
        );
    }

    pub fn insert_failure(&mut self, query: String, now: u64) {
        self.store(
            query,
            GeoCacheEntry {
                geo: None,
                fetched_at: now,
                failure: true,
            },
        );
    }

    pub fn merge_newer(&mut self, other: &GeoCache) {
        for (key, entry) in &other.entries {
            match self.entries.get(key) {
                Some(existing) if existing.fetched_at >= entry.fetched_at => {}
                _ => {
                    self.entries.insert(key.clone(), entry.clone());
                }
            }
        }
        self.loaded = true;
    }

    fn store(&mut self, query: String, entry: GeoCacheEntry) {
        if self.entries.len() >= MAX_CACHE_ENTRIES && !self.entries.contains_key(&query) {
            if let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, item)| item.fetched_at)
                .map(|(key, _)| key.clone())
            {
                self.entries.remove(&oldest);
            }
        }
        self.entries.insert(query, entry);
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn cache_path_for_config(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("geo-cache.json")
}

pub fn is_non_geo_ip(value: &str) -> bool {
    let Ok(ip) = value.parse::<IpAddr>() else {
        return false;
    };
    is_unusable_ip(ip)
}

fn is_unusable_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_private()
                || v4.is_link_local()
                || is_cgnat(v4)
                || is_fake_ip(v4)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
        }
    }
}

fn is_cgnat(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (octets[1] & 0b1100_0000) == 64
}

fn is_fake_ip(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 198 && (octets[1] == 18 || octets[1] == 19)
}

pub fn destination_geo_query(ip: &str, host: Option<&str>) -> Option<String> {
    let ip = ip.trim();
    if !ip.is_empty() && !is_non_geo_ip(ip) {
        return Some(ip.to_string());
    }

    let host = host.map(str::trim).filter(|value| !value.is_empty())?;
    if is_non_geo_ip(host) {
        return None;
    }
    Some(host.to_string())
}

pub fn proxy_geo_query(server: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() || is_non_geo_ip(server) {
        return None;
    }
    Some(server.to_string())
}

#[derive(Debug, Deserialize)]
struct IpApiItem {
    status: Option<String>,
    country: Option<String>,
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
    city: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
}

pub fn parse_ip_api_item(value: &serde_json::Value) -> Option<GeoLocation> {
    let item: IpApiItem = serde_json::from_value(value.clone()).ok()?;
    if item.status.as_deref() != Some("success") {
        return None;
    }
    let geo = GeoLocation {
        country: empty_to_none(item.country),
        country_code: empty_to_none(item.country_code),
        city: empty_to_none(item.city),
        latitude: item.lat,
        longitude: item.lon,
    };
    if geo.has_coordinates() {
        Some(geo)
    } else {
        None
    }
}

fn empty_to_none(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub async fn load_cache(path: &Path) -> GeoCache {
    match tokio::fs::read_to_string(path).await {
        Ok(text) => match serde_json::from_str::<GeoCache>(&text) {
            Ok(mut cache) => {
                cache.loaded = true;
                cache
            }
            Err(error) => {
                warn!(path = ?path, error = %error, "Failed to parse GeoIP cache");
                GeoCache {
                    loaded: true,
                    ..GeoCache::default()
                }
            }
        },
        Err(_) => GeoCache {
            loaded: true,
            ..GeoCache::default()
        },
    }
}

pub async fn save_cache(path: &Path, cache: &GeoCache) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::context("Failed to create GeoIP cache directory", e))?;
    }
    let payload = serde_json::to_string(cache)?;
    let temp_path = path.with_extension("tmp");
    tokio::fs::write(&temp_path, payload)
        .await
        .map_err(|e| AppError::context("Failed to write GeoIP cache", e))?;
    tokio::fs::rename(&temp_path, path)
        .await
        .map_err(|e| AppError::context("Failed to persist GeoIP cache", e))?;
    Ok(())
}

pub async fn resolve_queries(
    client: &reqwest::Client,
    cache: &mut GeoCache,
    queries: &[String],
    cache_path: &Path,
) -> HashMap<String, Option<GeoLocation>> {
    let now = now_unix();
    let mut result = HashMap::new();
    let mut missing = Vec::new();

    for query in queries {
        if query.is_empty() {
            continue;
        }
        if let Some(hit) = cache.get(query, now) {
            result.insert(query.clone(), hit);
        } else {
            missing.push(query.clone());
        }
    }

    if missing.is_empty() {
        return result;
    }

    if cfg!(test) {
        for query in missing {
            result.insert(query, None);
        }
        return result;
    }

    let RemoteLookup {
        resolved: looked_up,
        failed,
    } = lookup_remote(client, &missing).await;
    let mut changed = false;
    for query in missing {
        if let Some(geo) = looked_up.get(&query) {
            cache.insert(query.clone(), geo.clone(), now);
            result.insert(query, geo.clone());
            changed = true;
        } else {
            cache.insert_failure(query.clone(), now);
            result.insert(query, None);
            changed = true;
        }
    }
    if !failed.is_empty() {
        warn!(count = failed.len(), "GeoIP lookup failed for some queries");
    }
    if changed {
        if let Err(error) = save_cache(cache_path, cache).await {
            warn!(error = %error, "Failed to save GeoIP cache");
        }
    }

    result
}

struct RemoteLookup {
    resolved: HashMap<String, Option<GeoLocation>>,
    failed: Vec<String>,
}

async fn lookup_remote(client: &reqwest::Client, queries: &[String]) -> RemoteLookup {
    let mut resolved = HashMap::new();
    let mut failed = Vec::new();
    let mut remaining: Vec<String> = queries.to_vec();

    if let Some(index) = remaining.iter().position(|query| query == SELF_QUERY) {
        remaining.remove(index);
        match lookup_self(client).await {
            Ok(geo) => {
                resolved.insert(SELF_QUERY.to_string(), geo);
            }
            Err(error) => {
                warn!(error = %error, "Client GeoIP lookup failed");
                failed.push(SELF_QUERY.to_string());
            }
        }
    }

    let mut skip_remaining = false;
    for chunk in remaining.chunks(BATCH_SIZE) {
        if skip_remaining {
            failed.extend(chunk.iter().cloned());
            continue;
        }

        match lookup_batch(client, chunk).await {
            Ok(items) => {
                for (query, geo) in items {
                    resolved.insert(query, geo);
                }
            }
            Err(BatchError::RateLimited) => {
                warn!("GeoIP API rate-limited remaining batches");
                failed.extend(chunk.iter().cloned());
                skip_remaining = true;
            }
            Err(BatchError::Other(error)) => {
                warn!(error = %error, count = chunk.len(), "GeoIP batch failed");
                failed.extend(chunk.iter().cloned());
            }
        }
    }

    RemoteLookup { resolved, failed }
}

enum BatchError {
    RateLimited,
    Other(String),
}

async fn lookup_batch(
    client: &reqwest::Client,
    chunk: &[String],
) -> Result<Vec<(String, Option<GeoLocation>)>, BatchError> {
    let url = format!("http://ip-api.com/batch?fields={IP_API_FIELDS}");
    let response = client
        .post(url)
        .json(&chunk)
        .send()
        .await
        .map_err(|e| BatchError::Other(format!("Failed to query GeoIP API: {e}")))?;

    let status = response.status();
    if status.as_u16() == 429 {
        return Err(BatchError::RateLimited);
    }
    if !status.is_success() {
        return Err(BatchError::Other(format!("GeoIP API returned {status}")));
    }

    let items: Vec<serde_json::Value> = response
        .json()
        .await
        .map_err(|e| BatchError::Other(format!("Failed to parse GeoIP API response: {e}")))?;

    info!(count = chunk.len(), "Resolved GeoIP batch");
    Ok(chunk
        .iter()
        .zip(items)
        .map(|(query, item)| (query.clone(), parse_ip_api_item(&item)))
        .collect())
}

async fn lookup_self(client: &reqwest::Client) -> AppResult<Option<GeoLocation>> {
    let url = format!("http://ip-api.com/json/?fields={IP_API_FIELDS}");
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::context("Failed to query client GeoIP", e))?;
    if !response.status().is_success() {
        return Err(AppError::message(format!(
            "Client GeoIP API returned {}",
            response.status()
        )));
    }
    let item: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::context("Failed to parse client GeoIP response", e))?;
    Ok(parse_ip_api_item(&item))
}

/// 在 sing-box 启动前（流量还是直连时）无条件刷新本机定位并写入缓存。
/// TUN 一旦起来，self 查询会经代理出口，把“本机”定位到节点所在城市——
/// 所以这里总是重新查询，同时能覆盖掉此前被代理污染的旧缓存。
pub async fn refresh_self_geo(client: &reqwest::Client, cache: &mut GeoCache, cache_path: &Path) {
    let now = now_unix();
    match lookup_self(client).await {
        Ok(geo) => {
            cache.insert(SELF_QUERY.to_string(), geo, now);
        }
        Err(error) => {
            warn!(error = %error, "Client GeoIP refresh failed");
            cache.insert_failure(SELF_QUERY.to_string(), now);
        }
    }
    if let Err(error) = save_cache(cache_path, cache).await {
        warn!(error = %error, "Failed to save GeoIP cache");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cache_path_for_config, destination_geo_query, is_non_geo_ip, parse_ip_api_item,
        proxy_geo_query, GeoCache,
    };
    use crate::models::GeoLocation;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn private_and_fake_ips_are_not_geolocated() {
        assert!(is_non_geo_ip("10.0.0.8"));
        assert!(is_non_geo_ip("192.168.1.1"));
        assert!(is_non_geo_ip("172.18.0.1"));
        assert!(is_non_geo_ip("127.0.0.1"));
        assert!(is_non_geo_ip("198.18.0.12"));
        assert!(is_non_geo_ip("198.19.255.1"));
        assert!(is_non_geo_ip("100.64.1.2"));
        assert!(is_non_geo_ip("::1"));
        assert!(!is_non_geo_ip("1.1.1.1"));
        assert!(!is_non_geo_ip("youtube.com"));
    }

    #[test]
    fn destination_query_prefers_public_ip_then_host() {
        assert_eq!(
            destination_geo_query("142.250.1.1", Some("youtube.com")).as_deref(),
            Some("142.250.1.1")
        );
        assert_eq!(
            destination_geo_query("198.18.0.4", Some("googlevideo.com")).as_deref(),
            Some("googlevideo.com")
        );
        assert_eq!(destination_geo_query("10.0.0.2", Some("192.168.1.8")), None);
        assert_eq!(
            destination_geo_query("", Some("github.com")).as_deref(),
            Some("github.com")
        );
    }

    #[test]
    fn proxy_query_skips_blank_and_private_servers() {
        assert_eq!(
            proxy_geo_query("tokyo.example.com").as_deref(),
            Some("tokyo.example.com")
        );
        assert_eq!(proxy_geo_query("10.0.0.1"), None);
        assert_eq!(proxy_geo_query("  "), None);
    }

    #[test]
    fn parse_ip_api_item_reads_success_payload() {
        let geo = parse_ip_api_item(&json!({
            "status": "success",
            "country": "Germany",
            "countryCode": "DE",
            "city": "Frankfurt",
            "lat": 50.11,
            "lon": 8.68,
            "query": "142.250.1.1"
        }))
        .unwrap();

        assert_eq!(geo.city.as_deref(), Some("Frankfurt"));
        assert_eq!(geo.country_code.as_deref(), Some("DE"));
        assert_eq!(geo.latitude, Some(50.11));
    }

    #[test]
    fn parse_ip_api_item_rejects_failures() {
        assert!(
            parse_ip_api_item(&json!({ "status": "fail", "message": "private range" })).is_none()
        );
        assert!(parse_ip_api_item(&json!({ "status": "success", "city": "X" })).is_none());
    }

    #[test]
    fn cache_expires_misses_faster_than_hits() {
        let mut cache = GeoCache::default();
        let geo = GeoLocation {
            country: Some("JP".into()),
            country_code: Some("JP".into()),
            city: Some("Tokyo".into()),
            latitude: Some(35.0),
            longitude: Some(139.0),
        };
        cache.insert("1.1.1.1".into(), Some(geo), 1_000);
        cache.insert("missing.example".into(), None, 1_000);

        assert!(cache.get("1.1.1.1", 1_000 + 2 * 24 * 3600).is_some());
        assert!(cache.get("missing.example", 1_000 + 7 * 3600).is_none());
        assert!(cache.get("missing.example", 1_000 + 60).unwrap().is_none());
    }

    #[test]
    fn transport_failures_expire_faster_than_definitive_misses() {
        let mut cache = GeoCache::default();
        cache.insert_failure("self".into(), 1_000);

        assert!(cache.get("self", 1_000 + 30).unwrap().is_none());
        assert!(cache.get("self", 1_000 + 4 * 60).is_none());
    }

    #[test]
    fn merge_newer_keeps_the_fresher_entry() {
        let mut older = GeoCache::default();
        older.insert("1.1.1.1".into(), None, 1_000);
        let mut newer = GeoCache::default();
        let geo = GeoLocation {
            country: Some("AU".into()),
            country_code: Some("AU".into()),
            city: Some("Sydney".into()),
            latitude: Some(-33.8),
            longitude: Some(151.2),
        };
        newer.insert("1.1.1.1".into(), Some(geo.clone()), 2_000);
        newer.insert("8.8.8.8".into(), None, 2_000);

        older.merge_newer(&newer);
        assert_eq!(
            older
                .get("1.1.1.1", 2_000)
                .unwrap()
                .unwrap()
                .city
                .as_deref(),
            Some("Sydney")
        );
        assert!(older.get("8.8.8.8", 2_000).unwrap().is_none());
    }

    #[test]
    fn cache_path_follows_config_directory() {
        assert_eq!(
            cache_path_for_config(&PathBuf::from("/etc/miao/config.yaml")),
            PathBuf::from("/etc/miao/geo-cache.json")
        );
    }
}
