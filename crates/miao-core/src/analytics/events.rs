//! Best-effort indexing of text logs; unknown formats remain searchable raw logs.
//! DNS latency comes only from versioned kernel events, never context age.
use std::sync::OnceLock;

use duckdb::{params, Connection};
use serde::Deserialize;

use crate::models::analytics::*;

#[derive(Default)]
pub(super) struct ParsedLog {
    pub component: String,
    pub context: String,
    pub event: &'static str,
    pub route: String,
    pub dns: Option<DnsCompletion>,
}

#[derive(Deserialize)]
pub(super) struct DnsCompletion {
    v: u32,
    pub domain: String,
    pub query_type: String,
    pub duration_ms: f64,
    pub outcome: String,
    pub rcode: Option<i32>,
    pub error: Option<String>,
    pub transport: Option<String>,
}

pub(super) fn parse(message: &str, session: &str) -> ParsedLog {
    static ANSI: OnceLock<regex::Regex> = OnceLock::new();
    static PREFIX: OnceLock<regex::Regex> = OnceLock::new();
    let plain = ANSI
        .get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap())
        .replace_all(message, "");
    let prefix = PREFIX.get_or_init(|| regex::Regex::new(r"^(?:(?:[+-]\d{4} )?\d{4}-\d\d-\d\d \d\d:\d\d:\d\d )?(?:TRACE|DEBUG|INFO|WARN|ERROR|FATAL|PANIC)(?:\[\d+\])?\s+(?:\[(\d+) [^\]]+\]\s+)?(?:([a-zA-Z0-9_-]+(?:/[a-zA-Z0-9_-]+)?(?:\[[^\]\r\n]*\])?): )?(.*)$").unwrap());
    let Some(parts) = prefix.captures(plain.trim_end()) else {
        return ParsedLog {
            event: "unparsed",
            ..Default::default()
        };
    };
    let component = parts.get(2).map(|m| m.as_str()).unwrap_or("core");
    let body = parts.get(3).map(|m| m.as_str()).unwrap_or_default();
    let context = parts
        .get(1)
        .map(|id| format!("{session}/{}", id.as_str()))
        .unwrap_or_default();
    let dns = if component == "dns" {
        body.strip_prefix("MIAO_DNS ")
            .and_then(|json| serde_json::from_str::<DnsCompletion>(json).ok())
            .filter(|e| {
                e.v == 1
                    && e.duration_ms.is_finite()
                    && e.duration_ms >= 0.0
                    && matches!(e.outcome.as_str(), "success" | "dns_error" | "error")
            })
    } else {
        None
    };
    let event = if dns.is_some() {
        "dns.complete"
    } else if matches!(
        super::log_level(&plain),
        "warn" | "error" | "fatal" | "panic"
    ) {
        // An error such as "exchange failed" is not a query-start event.
        "other"
    } else if component == "dns" && body.starts_with("exchange ") {
        "dns.query"
    } else if component == "dns" && body.starts_with("lookup domain ") {
        "dns.lookup"
    } else if component == "dns" && body.starts_with("cached ") {
        "dns.cached_record"
    } else if component == "dns" && body.starts_with("optimistic ") {
        "dns.optimistic_record"
    } else if component == "dns" && body.starts_with("exchanged ") {
        "dns.answer"
    } else if component == "dns" && body.starts_with("refreshed ") {
        "dns.refresh"
    } else if component == "router" && body.starts_with("match[") {
        "route.match"
    } else if component == "router" && body.starts_with("sniffed ") {
        "route.sniff"
    } else if component.starts_with("outbound/")
        && (body.starts_with("outbound connection to ")
            || body.starts_with("outbound packet connection to "))
    {
        "outbound.connect"
    } else if component.starts_with("inbound/")
        && (body.starts_with("inbound connection from ")
            || body.starts_with("inbound packet connection from "))
    {
        "inbound.accept"
    } else {
        "other"
    };
    ParsedLog {
        component: component.into(),
        context,
        event,
        route: if event == "route.match" {
            body.into()
        } else {
            String::new()
        },
        dns,
    }
}

pub(super) fn activity(db: &Connection, since: i64, until: i64) -> duckdb::Result<LogActivity> {
    let (parsed_count, classified_count, contexts) = db.query_row("SELECT count(*) FILTER (WHERE event != 'unparsed'), count(*) FILTER (WHERE event NOT IN ('unparsed','other')), count(DISTINCT context) FILTER (WHERE context != '') FROM logs WHERE timestamp BETWEEN ? AND ?",
        params![since, until], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let mut facets = Vec::new();
    for dimension in ["component", "level", "event", "route"] {
        facets.extend(db.prepare(&format!("SELECT {dimension}, count(*) FROM logs WHERE timestamp BETWEEN ? AND ? AND {dimension} != '' GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT 20"))?
            .query_map(params![since, until], |r| Ok(LogFacet {dimension: dimension.into(), name: r.get(0)?, count: r.get(1)?}))?
            .collect::<duckdb::Result<Vec<_>>>()?);
    }
    // Rank each family independently; a busy DNS component must not hide a
    // low-volume inbound/outbound behind the global component Top 20.
    for family in ["inbound", "outbound"] {
        facets.extend(db.prepare("SELECT component, count(*) FROM logs WHERE timestamp BETWEEN ? AND ? AND starts_with(component, ?) GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT 20")?
            .query_map(params![since, until, format!("{family}/")], |r| Ok(LogFacet {dimension: family.into(), name: r.get(0)?, count: r.get(1)?}))?
            .collect::<duckdb::Result<Vec<_>>>()?);
    }
    let issues = db.prepare("SELECT component, level, count(*), count(DISTINCT context) FILTER (WHERE context != ''), max(timestamp) FROM logs WHERE timestamp BETWEEN ? AND ? AND level IN ('warn','error','fatal','panic') GROUP BY 1,2 ORDER BY 3 DESC,1,2 LIMIT 20")?
        .query_map(params![since, until], |r| Ok(LogIssueGroup {component: r.get(0)?, level: r.get(1)?, count: r.get(2)?, contexts: r.get(3)?, latest_at: r.get(4)?}))?
        .collect::<duckdb::Result<Vec<_>>>()?;
    Ok(LogActivity {
        parsed_count,
        classified_count,
        contexts,
        facets,
        issues,
    })
}

pub(super) fn dns(db: &Connection, since: i64, until: i64) -> duckdb::Result<DnsAnalytics> {
    let mut result = db.query_row("SELECT count(*), count(*) FILTER (WHERE outcome != 'success'), quantile_cont(duration_ms,0.5), quantile_cont(duration_ms,0.95), quantile_cont(duration_ms,0.99), max(duration_ms) FROM dns_events WHERE timestamp BETWEEN ? AND ?",
        params![since, until], |r| Ok(DnsAnalytics {queries: r.get(0)?, failures: r.get(1)?, p50_ms: r.get(2)?, p95_ms: r.get(3)?, p99_ms: r.get(4)?, max_ms: r.get(5)?, ..Default::default()}))?;
    result.timeline = db.prepare("SELECT (timestamp // 300000)*300000, count(*), count(*) FILTER (WHERE outcome != 'success'), quantile_cont(duration_ms,0.95), max(duration_ms) FROM dns_events WHERE timestamp BETWEEN ? AND ? GROUP BY 1 ORDER BY 1")?
        .query_map(params![since, until], |r| Ok(DnsBucket {timestamp: r.get(0)?, queries: r.get(1)?, failures: r.get(2)?, p95_ms: r.get(3)?, max_ms: r.get(4)?}))?
        .collect::<duckdb::Result<Vec<_>>>()?;
    for dimension in ["domain", "query_type", "transport", "outcome"] {
        result.groups.extend(db.prepare(&format!("SELECT coalesce({dimension},'unknown'), count(*), count(*) FILTER (WHERE outcome != 'success'), quantile_cont(duration_ms,0.95), max(duration_ms) FROM dns_events WHERE timestamp BETWEEN ? AND ? GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT 20"))?
            .query_map(params![since, until], |r| Ok(DnsGroup {dimension: dimension.into(), name: r.get(0)?, queries: r.get(1)?, failures: r.get(2)?, p95_ms: r.get(3)?, max_ms: r.get(4)?}))?
            .collect::<duckdb::Result<Vec<_>>>()?);
    }
    // These rankings run over all domains, not just the busiest 20 domains.
    for (order, having, target) in [
        ("5 DESC", "", &mut result.slow_domains),
        (
            "3 DESC",
            "HAVING count(*) FILTER (WHERE outcome != 'success') > 0",
            &mut result.failed_domains,
        ),
    ] {
        *target = db.prepare(&format!("SELECT domain, count(*), count(*) FILTER (WHERE outcome != 'success'), quantile_cont(duration_ms,0.95), max(duration_ms) FROM dns_events WHERE timestamp BETWEEN ? AND ? GROUP BY 1 {having} ORDER BY {order}, 1 LIMIT 20"))?
            .query_map(params![since, until], |r| Ok(DnsGroup {dimension: "domain".into(), name: r.get(0)?, queries: r.get(1)?, failures: r.get(2)?, p95_ms: r.get(3)?, max_ms: r.get(4)?}))?
            .collect::<duckdb::Result<Vec<_>>>()?;
    }
    result.slowest = db.prepare("SELECT log_id,timestamp,domain,query_type,duration_ms,outcome,rcode,error,transport,context FROM dns_events WHERE timestamp BETWEEN ? AND ? ORDER BY duration_ms DESC, log_id DESC LIMIT 20")?
        .query_map(params![since, until], |r| Ok(DnsEvent {log_id: r.get(0)?, timestamp: r.get(1)?, domain: r.get(2)?, query_type: r.get(3)?, duration_ms: r.get(4)?, outcome: r.get(5)?, rcode: r.get(6)?, error: r.get(7)?, transport: r.get(8)?, context: r.get(9)?}))?
        .collect::<duckdb::Result<Vec<_>>>()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_tags_preserve_spaces_unicode_and_colons() {
        let parsed = parse(
            "-0700 2026-09-23 12:00:00 INFO [7 2ms] outbound/trojan[日本 Tokyo: 01]: outbound connection to example.com:443",
            "run",
        );
        assert_eq!(parsed.component, "outbound/trojan[日本 Tokyo: 01]");
        assert_eq!(parsed.context, "run/7");
        assert_eq!(parsed.event, "outbound.connect");
        assert_eq!(
            parse("INFO clash-api: server started", "run").component,
            "clash-api"
        );
    }

    #[test]
    fn context_age_is_never_a_dns_duration_and_records_are_not_cache_hits() {
        let parsed = parse(
            "+0900 2026-09-23 12:00:00 DEBUG [42 17.5s] dns: cached A example.com. 60 IN A 1.2.3.4",
            "run-1",
        );
        assert_eq!(parsed.context, "run-1/42");
        assert_eq!(parsed.event, "dns.cached_record");
        assert!(parsed.dns.is_none());
        assert_ne!(
            parsed.context,
            parse("DEBUG [42 1ms] dns: query", "run-2").context
        );
        let untrusted = parse(
            "INFO outbound/direct: text dns: MIAO_DNS {\"v\":1}",
            "run-1",
        );
        assert!(untrusted.dns.is_none());
        let route = parse(
            "DEBUG [5 0ms] router: match[2] domain_suffix=.test => route(direct)",
            "r",
        );
        assert_eq!(route.event, "route.match");
        assert_eq!(route.route, "match[2] domain_suffix=.test => route(direct)");
        assert_eq!(parse("future format", "r").event, "unparsed");
    }
}
