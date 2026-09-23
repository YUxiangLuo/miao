use serde::Serialize;

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AnalyticsLog {
    pub id: i64,
    pub timestamp: i64,
    pub stream: String,
    pub level: String,
    pub message: String,
    pub component: String,
    pub event: String,
    pub context: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AnalyticsLogs {
    pub entries: Vec<AnalyticsLog>,
    pub next_before: Option<i64>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AnalyticsBucket {
    pub timestamp: i64,
    pub upload: i64,
    pub download: i64,
    pub log_count: i64,
    pub error_count: i64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AnalyticsGroup {
    pub dimension: String,
    pub name: String,
    pub connections: i64,
    pub upload: i64,
    pub download: i64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AnalyticsSummary {
    pub since: i64,
    pub until: i64,
    pub log_count: i64,
    pub error_count: i64,
    pub upload: i64,
    pub download: i64,
    pub sampled_connections: i64,
    pub last_sample_at: Option<i64>,
    pub collection_error: Option<String>,
    pub timeline: Vec<AnalyticsBucket>,
    pub groups: Vec<AnalyticsGroup>,
    pub activity: LogActivity,
    pub dns: DnsAnalytics,
}

#[derive(Debug, Serialize, Default)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogActivity {
    pub parsed_count: i64,
    pub classified_count: i64,
    pub contexts: i64,
    pub facets: Vec<LogFacet>,
    pub issues: Vec<LogIssueGroup>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogIssueGroup {
    pub component: String,
    pub level: String,
    pub count: i64,
    pub contexts: i64,
    pub latest_at: i64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LogFacet {
    pub dimension: String,
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Default)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DnsAnalytics {
    pub queries: i64,
    pub failures: i64,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub timeline: Vec<DnsBucket>,
    pub groups: Vec<DnsGroup>,
    pub slow_domains: Vec<DnsGroup>,
    pub failed_domains: Vec<DnsGroup>,
    pub slowest: Vec<DnsEvent>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DnsBucket {
    pub timestamp: i64,
    pub queries: i64,
    pub failures: i64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DnsGroup {
    pub dimension: String,
    pub name: String,
    pub queries: i64,
    pub failures: i64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DnsEvent {
    pub log_id: i64,
    pub timestamp: i64,
    pub domain: String,
    pub query_type: String,
    pub duration_ms: f64,
    pub outcome: String,
    pub rcode: Option<i32>,
    pub error: Option<String>,
    pub transport: Option<String>,
    pub context: String,
}
