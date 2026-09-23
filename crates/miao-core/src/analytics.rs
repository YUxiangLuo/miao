//! Local 24-hour history. All DuckDB work runs off Tokio's executor threads.
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use duckdb::{params, Connection};

use crate::models::analytics::*;

mod collector;
mod events;
pub(crate) use collector::{capture, spawn_sampler};

pub const RETENTION_MS: i64 = 24 * 60 * 60 * 1000;
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub struct Analytics {
    db: Mutex<Connection>,
    reader: Mutex<Connection>,
    // Keep a log write failure visible even if connection sampling succeeds.
    pub log_error: Mutex<Option<String>>,
    pub sample_error: Mutex<Option<String>>,
}

pub struct LogRecord {
    pub timestamp: i64,
    pub stream: &'static str,
    pub session: String,
    pub raw: Vec<u8>,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
pub struct LogFilter {
    pub hours: Option<i64>,
    pub level: String,
    pub q: String,
    pub before: Option<i64>,
    pub limit: Option<usize>,
    pub component: String,
    pub event: String,
    pub context: String,
    pub id: Option<i64>,
}

#[derive(Default, Debug)]
pub struct ConnectionDelta {
    pub id: String,
    pub domain: String,
    pub source: String,
    pub process: String,
    pub outbound: String,
    pub rule: String,
    pub network: String,
    pub upload: i64,
    pub download: i64,
}

impl Analytics {
    pub fn open(path: &Path) -> Result<Arc<Self>, String> {
        let parent = path
            .parent()
            .ok_or("Analytics database needs a directory")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        Self::initialize(db)
            .map(Arc::new)
            .map_err(|e| e.to_string())
    }

    fn initialize(mut db: Connection) -> duckdb::Result<Self> {
        db.execute_batch(
            "SET enable_external_access = false;
            SET threads = 2;
            SET memory_limit = '512MB';
            CREATE SEQUENCE IF NOT EXISTS log_id;
            CREATE TABLE IF NOT EXISTS logs (
                id BIGINT DEFAULT nextval('log_id'), timestamp BIGINT NOT NULL,
                stream VARCHAR NOT NULL, level VARCHAR NOT NULL,
                message VARCHAR NOT NULL, raw BLOB NOT NULL,
                component VARCHAR NOT NULL, event VARCHAR NOT NULL, context VARCHAR NOT NULL, route VARCHAR NOT NULL);
            CREATE TABLE IF NOT EXISTS dns_events (
                log_id BIGINT NOT NULL, timestamp BIGINT NOT NULL, domain VARCHAR NOT NULL,
                query_type VARCHAR NOT NULL, duration_ms DOUBLE NOT NULL, outcome VARCHAR NOT NULL,
                rcode INTEGER, error VARCHAR, transport VARCHAR, context VARCHAR NOT NULL);
            CREATE TABLE IF NOT EXISTS samples (timestamp BIGINT NOT NULL);
            CREATE TABLE IF NOT EXISTS traffic (
                timestamp BIGINT NOT NULL, id VARCHAR NOT NULL,
                domain VARCHAR NOT NULL, source VARCHAR NOT NULL, process VARCHAR NOT NULL,
                outbound VARCHAR NOT NULL, rule VARCHAR NOT NULL, network VARCHAR NOT NULL,
                upload BIGINT NOT NULL, download BIGINT NOT NULL);",
        )?;
        Self::prune(&mut db, now_ms())?;
        let reader = db.try_clone()?;
        Ok(Self {
            db: Mutex::new(db),
            reader: Mutex::new(reader),
            log_error: Mutex::new(None),
            sample_error: Mutex::new(None),
        })
    }

    pub async fn run<T, F>(self: &Arc<Self>, work: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> duckdb::Result<T> + Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            work(&mut store.db.lock().unwrap()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub async fn read<T, F>(self: &Arc<Self>, work: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> duckdb::Result<T> + Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut db = store.reader.lock().unwrap();
            // One bounded reader lane, separate from capture/pruning. A summary
            // runs multiple SELECTs; they must all see the same committed state.
            let tx = db.transaction().map_err(|e| e.to_string())?;
            let result = work(&tx).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(result)
        })
        .await
        .map_err(|e| e.to_string())?
    }

    pub fn insert_logs(db: &mut Connection, rows: &[LogRecord]) -> duckdb::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let tx = db.transaction()?;
        {
            // Allocate from the existing sequence in one query. Sort the IDs so
            // log pagination retains input order regardless of query parallelism.
            // Rollbacks may consume IDs, just as with INSERT ... RETURNING.
            let mut ids = tx
                .prepare("SELECT nextval('log_id') FROM range(?)")?
                .query_map([rows.len() as i64], |r| r.get::<_, i64>(0))?
                .collect::<duckdb::Result<Vec<_>>>()?;
            ids.sort_unstable();
            let mut insert = tx.appender("logs")?;
            let mut dns_insert = tx.appender("dns_events")?;
            for (row, id) in rows.iter().zip(ids) {
                let message = String::from_utf8_lossy(&row.raw);
                let parsed = events::parse(&message, &row.session);
                insert.append_row(params![
                    id,
                    row.timestamp,
                    row.stream,
                    log_level(&message),
                    message.as_ref(),
                    row.raw,
                    parsed.component,
                    parsed.event,
                    parsed.context,
                    parsed.route
                ])?;
                if let Some(dns) = parsed.dns {
                    dns_insert.append_row(params![
                        id,
                        row.timestamp,
                        dns.domain,
                        dns.query_type,
                        dns.duration_ms,
                        dns.outcome,
                        dns.rcode,
                        dns.error,
                        dns.transport,
                        parsed.context
                    ])?;
                }
            }
            // Drop flushes too, but discards errors. Check both flushes before
            // committing so a failure in either table rolls back the whole batch.
            insert.flush()?;
            dns_insert.flush()?;
        }
        tx.commit()
    }

    pub fn insert_sample(
        db: &mut Connection,
        timestamp: i64,
        rows: &[ConnectionDelta],
    ) -> duckdb::Result<()> {
        let tx = db.transaction()?;
        tx.execute("INSERT INTO samples VALUES (?)", [timestamp])?;
        {
            let mut insert =
                tx.prepare("INSERT INTO traffic VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")?;
            for row in rows {
                insert.execute(params![
                    timestamp,
                    row.id,
                    row.domain,
                    row.source,
                    row.process,
                    row.outbound,
                    row.rule,
                    row.network,
                    row.upload,
                    row.download
                ])?;
            }
        }
        tx.commit()
    }

    pub fn prune(db: &mut Connection, now: i64) -> duckdb::Result<()> {
        // Queries also enforce retention, so records cannot leak between sweeps.
        // Publish cleanup across all tables atomically to concurrent readers.
        let tx = db.transaction()?;
        for table in ["logs", "dns_events", "samples", "traffic"] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE timestamp < ?"),
                [now - RETENTION_MS],
            )?;
        }
        tx.commit()?;
        db.execute_batch("CHECKPOINT")
    }

    pub fn logs(
        db: &Connection,
        since: i64,
        until: i64,
        filter: &LogFilter,
    ) -> duckdb::Result<AnalyticsLogs> {
        let limit = filter.limit.unwrap_or(100);
        let mut stmt = db.prepare(
            "SELECT id, timestamp, stream, level, message, component, event, context FROM logs
            WHERE timestamp >= ? AND timestamp <= ? AND id < ?
            AND (? = '' OR level = ?) AND contains(lower(message), lower(?))
            AND (? = '' OR component = ?) AND (? = '' OR event = ?) AND (? = '' OR context = ?)
            AND (? IS NULL OR id = ?)
            ORDER BY id DESC LIMIT ?",
        )?;
        let mut entries = stmt
            .query_map(
                params![
                    since,
                    until,
                    filter.before.unwrap_or(i64::MAX),
                    filter.level,
                    filter.level,
                    filter.q,
                    filter.component,
                    filter.component,
                    filter.event,
                    filter.event,
                    filter.context,
                    filter.context,
                    filter.id,
                    filter.id,
                    (limit + 1) as i64
                ],
                |row| {
                    Ok(AnalyticsLog {
                        id: row.get(0)?,
                        timestamp: row.get(1)?,
                        stream: row.get(2)?,
                        level: row.get(3)?,
                        message: row.get(4)?,
                        component: row.get(5)?,
                        event: row.get(6)?,
                        context: row.get(7)?,
                    })
                },
            )?
            .collect::<duckdb::Result<Vec<_>>>()?;
        let next_before = if entries.len() > limit {
            entries.truncate(limit);
            entries.last().map(|row| row.id)
        } else {
            None
        };
        Ok(AnalyticsLogs {
            entries,
            next_before,
        })
    }

    pub fn summary(db: &Connection, since: i64, until: i64) -> duckdb::Result<AnalyticsSummary> {
        let (log_count, error_count) = db.query_row("SELECT count(*), count(*) FILTER (WHERE level IN ('error','fatal','panic')) FROM logs WHERE timestamp BETWEEN ? AND ?",
            params![since, until], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let (upload, download, sampled_connections) = db.query_row("SELECT coalesce(sum(upload),0)::BIGINT, coalesce(sum(download),0)::BIGINT, count(DISTINCT id) FROM traffic WHERE timestamp BETWEEN ? AND ?",
            params![since, until], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        let last_sample_at = db.query_row(
            "SELECT max(timestamp) FROM samples WHERE timestamp BETWEEN ? AND ?",
            params![until - RETENTION_MS, until],
            |r| r.get(0),
        )?;
        let mut groups = Vec::new();
        // Identifiers are a fixed allowlist, never user-provided SQL.
        for dimension in ["domain", "source", "process", "outbound", "rule", "network"] {
            let sql = format!("SELECT {dimension}, count(DISTINCT id), coalesce(sum(upload),0)::BIGINT, coalesce(sum(download),0)::BIGINT FROM traffic
                WHERE timestamp BETWEEN ? AND ? GROUP BY {dimension} ORDER BY sum(upload)+sum(download) DESC, {dimension} LIMIT 20");
            groups.extend(
                db.prepare(&sql)?
                    .query_map(params![since, until], |r| {
                        Ok(AnalyticsGroup {
                            dimension: dimension.into(),
                            name: r.get(0)?,
                            connections: r.get(1)?,
                            upload: r.get(2)?,
                            download: r.get(3)?,
                        })
                    })?
                    .collect::<duckdb::Result<Vec<_>>>()?,
            );
        }
        let mut timeline = std::collections::BTreeMap::new();
        const BUCKET: i64 = 300_000;
        // The selected window is a maximum lookback, not proof we observed
        // the entire period. Include zero-traffic samples as real observations.
        let first_record: Option<i64> = db.query_row(
            "SELECT min(timestamp) FROM (SELECT timestamp FROM logs WHERE timestamp BETWEEN ? AND ? UNION ALL SELECT timestamp FROM samples WHERE timestamp BETWEEN ? AND ?)",
            params![since, until, since, until],
            |r| r.get(0),
        )?;
        if let Some(first_record) = first_record {
            for timestamp in ((first_record / BUCKET * BUCKET)..=until).step_by(BUCKET as usize) {
                timeline.insert(
                    timestamp,
                    AnalyticsBucket {
                        timestamp,
                        upload: 0,
                        download: 0,
                        log_count: 0,
                        error_count: 0,
                    },
                );
            }
        }
        for bucket in db.prepare("SELECT (timestamp // 300000) * 300000, sum(upload)::BIGINT, sum(download)::BIGINT FROM traffic WHERE timestamp BETWEEN ? AND ? GROUP BY 1")?
            .query_map(params![since, until], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
            let (t, up, down) = bucket?;
            if let Some(b) = timeline.get_mut(&t) { b.upload = up; b.download = down; }
        }
        for bucket in db.prepare("SELECT (timestamp // 300000) * 300000, count(*), count(*) FILTER (WHERE level IN ('error','fatal','panic')) FROM logs WHERE timestamp BETWEEN ? AND ? GROUP BY 1")?
            .query_map(params![since, until], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))? {
            let (t, count, errors) = bucket?;
            if let Some(b) = timeline.get_mut(&t) { b.log_count = count; b.error_count = errors; }
        }
        Ok(AnalyticsSummary {
            since,
            until,
            log_count,
            error_count,
            upload,
            download,
            sampled_connections,
            last_sample_at,
            collection_error: None,
            timeline: timeline.into_values().collect(),
            groups,
            activity: events::activity(db, since, until)?,
            dns: events::dns(db, since, until)?,
        })
    }
}

fn log_level(message: &str) -> &str {
    // sing-box prefix: [optional timestamp] LEVEL [context] message. Do not
    // infer severity from words like "error" occurring in the message body.
    static PREFIX: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let prefix = PREFIX.get_or_init(|| regex::Regex::new(r"^(?:(?:\x1b\[[0-9;]*m)|[+-]\d{4} |\d{4}-\d\d-\d\d |\d\d:\d\d:\d\d |\[\d+\] )*(TRACE|DEBUG|INFO|WARN|ERROR|FATAL|PANIC)(?:\b)").unwrap());
    match prefix
        .captures(message)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
    {
        Some("TRACE") => "trace",
        Some("DEBUG") => "debug",
        Some("INFO") => "info",
        Some("WARN") => "warn",
        Some("ERROR") => "error",
        Some("FATAL") => "fatal",
        Some("PANIC") => "panic",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests;
