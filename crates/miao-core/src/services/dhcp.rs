use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DhcpLease {
    pub ip: String,
    pub mac: String,
    pub hostname: Option<String>,
}

const DEFAULT_LEASE_PATHS: &[&str] = &["/tmp/dhcp.leases", "/var/lib/misc/dnsmasq.leases"];

fn normalize_mac(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut normalized = Vec::with_capacity(6);
    for part in parts {
        if part.len() != 2 || !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        normalized.push(part.to_ascii_lowercase());
    }
    Some(normalized.join(":"))
}

fn normalize_hostname(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return None;
    }
    let clean: String = trimmed
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    (!clean.is_empty()).then_some(clean)
}

/// 解析 OpenWrt / dnsmasq 租约文件（格式：`<expiry_epoch> <mac> <ip> <hostname> [client_id]`）。
/// 同一 IP 出现多次时保留过期时间戳更新的记录，并按 IPv4 地址升序输出。
pub fn parse_dnsmasq_leases(content: &str) -> Vec<DhcpLease> {
    let mut by_ip: BTreeMap<Ipv4Addr, (u64, DhcpLease)> = BTreeMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split_whitespace();
        let Some(expiry_str) = cols.next() else {
            continue;
        };
        let Some(mac_str) = cols.next() else {
            continue;
        };
        let Some(ip_str) = cols.next() else {
            continue;
        };
        let hostname_str = cols.next().unwrap_or("*");

        let expiry = expiry_str.parse::<u64>().unwrap_or(0);
        let Some(mac) = normalize_mac(mac_str) else {
            continue;
        };
        let Ok(ipv4) = ip_str.parse::<Ipv4Addr>() else {
            continue;
        };
        if ipv4.is_unspecified() || ipv4.is_broadcast() || ipv4.is_loopback() {
            continue;
        }
        let lease = DhcpLease {
            ip: ipv4.to_string(),
            mac,
            hostname: normalize_hostname(hostname_str),
        };
        match by_ip.get(&ipv4) {
            Some((prev_expiry, _)) if *prev_expiry > expiry => {}
            _ => {
                by_ip.insert(ipv4, (expiry, lease));
            }
        }
    }

    by_ip.into_values().map(|(_, lease)| lease).collect()
}

pub async fn discover_dhcp_leases() -> Vec<DhcpLease> {
    discover_dhcp_leases_from(DEFAULT_LEASE_PATHS).await
}

pub async fn discover_dhcp_leases_from(paths: &[impl AsRef<Path>]) -> Vec<DhcpLease> {
    for path in paths {
        if let Ok(content) = tokio::fs::read_to_string(path.as_ref()).await {
            let leases = parse_dnsmasq_leases(&content);
            if !leases.is_empty() {
                return leases;
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{discover_dhcp_leases_from, parse_dnsmasq_leases};

    #[test]
    fn parses_real_openwrt_dnsmasq_leases_and_sorts_by_ipv4() {
        let sample = "\
1795181572 00:E0:4D:A4:0C:35 192.168.8.214 CAIWU 01:00:e0:4d:a4:0c:35
1795183938 80:c5:f2:2b:e3:6c 192.168.8.180 YY 01:80:c5:f2:2b:e3:6c
1795184199 00:e0:1a:b1:51:78 192.168.8.242 * 01:00:e0:1a:b1:51:78
1795181253 16:aa:95:cf:be:57 192.168.8.229 iPhone 01:16:aa:95:cf:be:57
invalid line
";
        let leases = parse_dnsmasq_leases(sample);
        assert_eq!(leases.len(), 4);
        assert_eq!(leases[0].ip, "192.168.8.180");
        assert_eq!(leases[0].hostname.as_deref(), Some("YY"));
        assert_eq!(leases[1].ip, "192.168.8.214");
        assert_eq!(leases[1].mac, "00:e0:4d:a4:0c:35");
        assert_eq!(leases[1].hostname.as_deref(), Some("CAIWU"));
        assert_eq!(leases[2].ip, "192.168.8.229");
        assert_eq!(leases[2].hostname.as_deref(), Some("iPhone"));
        assert_eq!(leases[3].ip, "192.168.8.242");
        assert!(leases[3].hostname.is_none());
    }

    #[test]
    fn deduplicates_same_ip_keeping_newer_expiry() {
        let sample = "\
100 00:11:22:33:44:55 192.168.8.50 old-host *
200 aa:bb:cc:dd:ee:ff 192.168.8.50 new-host *
150 11:22:33:44:55:66 192.168.8.50 stale-host *
";
        let leases = parse_dnsmasq_leases(sample);
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].mac, "aa:bb:cc:dd:ee:ff");
        assert_eq!(leases[0].hostname.as_deref(), Some("new-host"));
    }

    #[tokio::test]
    async fn falls_back_across_paths_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.leases");
        let fallback = dir.path().join("dnsmasq.leases");
        tokio::fs::write(
            &fallback,
            "1795181572 00:e0:4d:a4:0c:35 192.168.8.214 CAIWU *\n",
        )
        .await
        .unwrap();

        let leases = discover_dhcp_leases_from(&[&missing, &fallback]).await;
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].ip, "192.168.8.214");
    }
}
