//! IP 地理定位:内嵌 MaxMind 格式(mmdb)城市数据库,纯本地查询,不访问任何外部 API。
//! 数据库文件随二进制内嵌,启动时由 singbox 模块统一释放到运行时目录。

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Mutex;

use maxminddb::Reader;
use tracing::warn;

use crate::services::singbox::get_sing_box_home;

pub const GEO_DB_FILENAME: &str = "geoip-city.mmdb";
const CACHE_CAPACITY: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lng: f64,
    pub country: Option<String>,
    pub city: Option<String>,
}

/// 地理数据库查询器。数据库不可用时降级为「无定位能力」(所有 lookup 返回 None),
/// 不阻塞主流程——CI 用空壳 stub 文件时走的正是这条路径。
pub struct GeoIp {
    reader: Option<Reader<Vec<u8>>>,
    cache: Mutex<HashMap<IpAddr, Option<GeoPoint>>>,
}

impl GeoIp {
    /// 打开运行时目录中已释放的数据库
    pub fn open_default() -> Self {
        Self::open(&get_sing_box_home().join(GEO_DB_FILENAME))
    }

    pub fn open(path: &Path) -> Self {
        let reader = std::fs::read(path)
            .ok()
            .and_then(|bytes| match Reader::from_source(bytes) {
                Ok(reader) => Some(reader),
                Err(err) => {
                    warn!(path = ?path, error = %err, "Geo database is invalid; IP geolocation disabled");
                    None
                }
            });
        if reader.is_none() {
            warn!(path = ?path, "Geo database unavailable; IP geolocation disabled");
        }
        Self {
            reader,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn lookup_str(&self, raw: &str) -> Option<GeoPoint> {
        raw.trim()
            .parse::<IpAddr>()
            .ok()
            .and_then(|ip| self.lookup(ip))
    }

    pub fn lookup(&self, ip: IpAddr) -> Option<GeoPoint> {
        let ip = normalize_ip(ip);
        if !is_locatable(ip) {
            return None;
        }
        if let Ok(cache) = self.cache.lock() {
            if let Some(cached) = cache.get(&ip) {
                return cached.clone();
            }
        }
        let point = self.lookup_uncached(ip);
        if let Ok(mut cache) = self.cache.lock() {
            // 容量触顶整体清空:连接目的 IP 重复率高,简单清理比精确淘汰性价比更高
            if cache.len() >= CACHE_CAPACITY {
                cache.clear();
            }
            cache.insert(ip, point.clone());
        }
        point
    }

    fn lookup_uncached(&self, ip: IpAddr) -> Option<GeoPoint> {
        let reader = self.reader.as_ref()?;
        // 用 Value 兼容 GeoLite2 与 DB-IP 两种 schema(字段路径一致但细节有差异)
        let value: Option<serde_json::Value> = reader.lookup(ip).ok().flatten();
        let value = value?;
        let lat = value.pointer("/location/latitude")?.as_f64()?;
        let lng = value.pointer("/location/longitude")?.as_f64()?;
        let country = value
            .pointer("/country/names/en")
            .and_then(|v| v.as_str())
            .or_else(|| value.pointer("/country/iso_code").and_then(|v| v.as_str()))
            .map(str::to_string);
        let city = value
            .pointer("/city/names/en")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Some(GeoPoint {
            lat,
            lng,
            country,
            city,
        })
    }
}

/// IPv4-mapped IPv6 归一化,保证 mmdb 查询与缓存键一致
fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

/// 私网/保留地址没有地理意义,直接排除
pub(crate) fn is_locatable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 100.64.0.0/10 CGNAT(Ipv4Addr::is_shared 尚未稳定)
            let is_cgnat = octets[0] == 100 && (octets[1] & 0xC0) == 64;
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || is_cgnat)
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (first & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (first & 0xffc0) == 0xfe80) // fe80::/10 link-local
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn is_locatable_rejects_private_and_reserved_ipv4() {
        assert!(!is_locatable(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
        assert!(!is_locatable(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
        assert!(!is_locatable(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(!is_locatable(IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))));
        assert!(is_locatable(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    }

    #[test]
    fn is_locatable_rejects_ula_and_link_local_ipv6() {
        assert!(!is_locatable(IpAddr::V6(
            "fd00::1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_locatable(IpAddr::V6(
            "fe80::1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(is_locatable(IpAddr::V6(
            "2606:4700:4700::1111".parse::<Ipv6Addr>().unwrap()
        )));
    }

    #[test]
    fn normalize_ip_maps_ipv4_mapped_ipv6() {
        let mapped: IpAddr = "::ffff:8.8.8.8".parse().unwrap();
        assert_eq!(normalize_ip(mapped), IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn missing_database_disables_lookup_without_error() {
        let geo = GeoIp::open(Path::new("/nonexistent/geoip-city.mmdb"));
        assert!(geo.lookup_str("8.8.8.8").is_none());
    }

    #[test]
    fn empty_database_file_disables_lookup_without_error() {
        // 与 CI quality 流水线的空壳 stub 文件行为一致
        let path = std::env::temp_dir().join(format!("miao-empty-{}.mmdb", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        let geo = GeoIp::open(&path);
        assert!(geo.lookup_str("8.8.8.8").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lookup_rejects_invalid_ip_string() {
        let geo = GeoIp::open(Path::new("/nonexistent/geoip-city.mmdb"));
        assert!(geo.lookup_str("not-an-ip").is_none());
    }
}
