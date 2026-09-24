//! AI metadata shares the network database and serialized connection lanes.
use super::{now_ms, Analytics};
use crate::models::ai::{AiAnalytics, AiRequestRecord, AiUsageGroup};
use duckdb::{params, Connection};
use serde::Deserialize;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;
pub(super) const MAX_PENDING: usize = 256;

pub(super) fn initialize(db: &Connection) -> duckdb::Result<()> {
    db.execute_batch("CREATE SEQUENCE IF NOT EXISTS ai_request_id;
        CREATE TABLE IF NOT EXISTS ai_requests (
        id BIGINT DEFAULT nextval('ai_request_id'), timestamp BIGINT NOT NULL,
        provider VARCHAR NOT NULL, model VARCHAR NOT NULL, upstream_model VARCHAR NOT NULL,
        endpoint VARCHAR NOT NULL, streaming BOOLEAN NOT NULL, outcome VARCHAR NOT NULL,
        http_status INTEGER, duration_ms BIGINT NOT NULL, first_byte_ms BIGINT,
        input_tokens BIGINT, output_tokens BIGINT, total_tokens BIGINT, usage_complete BOOLEAN NOT NULL);")
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub days: Option<i64>,
    pub provider: String,
    pub model: String,
    pub outcome: String,
    pub before: Option<i64>,
}

impl Analytics {
    /// Bounded best-effort writes: inference never waits for a database lock.
    pub fn record_ai(self: &Arc<Self>, row: AiRequestRecord) {
        let Ok(permit) = self.ai_pending.clone().try_acquire_owned() else {
            self.ai_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let store = self.clone();
        tokio::spawn(async move {
            if store.run(move |db| insert(db, &row)).await.is_err() {
                store.ai_dropped.fetch_add(1, Ordering::Relaxed);
                // Do not expose database paths or error strings through the UI.
                *store.ai_error.lock().unwrap() =
                    Some("AI 统计写入失败，部分请求记录已丢失".into());
            }
            drop(permit);
        });
    }

    pub async fn drain_ai(self: &Arc<Self>) {
        if tokio::time::timeout(
            Duration::from_secs(5),
            self.ai_pending
                .clone()
                .acquire_many_owned(MAX_PENDING as u32),
        )
        .await
        .is_err()
        {
            tracing::warn!("AI statistics did not drain; pending records may be lost");
        }
    }

    pub async fn ai_summary(self: &Arc<Self>, filter: Filter) -> Result<AiAnalytics, String> {
        let until = now_ms();
        let since = until - filter.days.unwrap_or(1).clamp(1, 30) * 86400000;
        let mut result = self
            .read(move |db| query(db, since, until, &filter))
            .await?;
        result.dropped_records = self.ai_dropped.load(Ordering::Relaxed);
        result.collection_error = self.ai_error.lock().unwrap().clone();
        Ok(result)
    }
}

fn insert(db: &Connection, r: &AiRequestRecord) -> duckdb::Result<()> {
    db.execute(
        "INSERT INTO ai_requests VALUES (nextval('ai_request_id'),?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            r.timestamp,
            r.provider,
            r.model,
            r.upstream_model,
            r.endpoint,
            r.streaming,
            r.outcome,
            r.http_status,
            r.duration_ms,
            r.first_byte_ms,
            r.input_tokens,
            r.output_tokens,
            r.total_tokens,
            r.usage_complete
        ],
    )?;
    Ok(())
}

const WHERE: &str = "timestamp >= ? AND timestamp < ? AND (? = '' OR provider = ?) AND (? = '' OR model = ?) AND (? = '' OR outcome = ?)";
const COUNTS: &str = "count(*)::BIGINT,
    count(*) FILTER (WHERE outcome = 'success')::BIGINT,
    count(*) FILTER (WHERE outcome NOT IN ('success','cancelled'))::BIGINT,
    count(*) FILTER (WHERE outcome = 'cancelled')::BIGINT,
    count(*) FILTER (WHERE usage_complete)::BIGINT,
    coalesce(sum(input_tokens) FILTER (WHERE usage_complete),0)::BIGINT,
    coalesce(sum(output_tokens) FILTER (WHERE usage_complete),0)::BIGINT,
    coalesce(sum(total_tokens) FILTER (WHERE usage_complete),0)::BIGINT,
    avg(duration_ms), quantile_cont(duration_ms,0.95)";

fn group(r: &duckdb::Row<'_>) -> duckdb::Result<AiUsageGroup> {
    Ok(AiUsageGroup {
        provider: r.get(0)?,
        model: r.get(1)?,
        requests: r.get(2)?,
        succeeded: r.get(3)?,
        failed: r.get(4)?,
        cancelled: r.get(5)?,
        usage_known: r.get(6)?,
        input_tokens: r.get(7)?,
        output_tokens: r.get(8)?,
        total_tokens: r.get(9)?,
        avg_duration_ms: r.get(10)?,
        p95_duration_ms: r.get(11)?,
    })
}

fn query(db: &Connection, since: i64, until: i64, f: &Filter) -> duckdb::Result<AiAnalytics> {
    let args =
        params![since, until, f.provider, f.provider, f.model, f.model, f.outcome, f.outcome];
    let totals = db.query_row(
        &format!("SELECT '', '', {COUNTS} FROM ai_requests WHERE {WHERE}"),
        args,
        group,
    )?;
    let groups = db.prepare(&format!("SELECT provider, model, {COUNTS} FROM ai_requests WHERE {WHERE} GROUP BY provider, model ORDER BY count(*) DESC, provider, model LIMIT 100"))?
        .query_map(args, group)?.collect::<duckdb::Result<Vec<_>>>()?;
    let mut records = db
        .prepare(&format!(
            "SELECT * FROM ai_requests WHERE {WHERE} AND id < ? ORDER BY id DESC LIMIT 101"
        ))?
        .query_map(
            params![
                since,
                until,
                f.provider,
                f.provider,
                f.model,
                f.model,
                f.outcome,
                f.outcome,
                f.before.unwrap_or(i64::MAX)
            ],
            |r| {
                Ok(AiRequestRecord {
                    id: r.get(0)?,
                    timestamp: r.get(1)?,
                    provider: r.get(2)?,
                    model: r.get(3)?,
                    upstream_model: r.get(4)?,
                    endpoint: r.get(5)?,
                    streaming: r.get(6)?,
                    outcome: r.get(7)?,
                    http_status: r.get(8)?,
                    duration_ms: r.get(9)?,
                    first_byte_ms: r.get(10)?,
                    input_tokens: r.get(11)?,
                    output_tokens: r.get(12)?,
                    total_tokens: r.get(13)?,
                    usage_complete: r.get(14)?,
                })
            },
        )?
        .collect::<duckdb::Result<Vec<_>>>()?;
    let next_before = if records.len() > 100 {
        records.truncate(100);
        records.last().map(|r| r.id)
    } else {
        None
    };
    Ok(AiAnalytics {
        since,
        until,
        totals,
        groups,
        records,
        next_before,
        dropped_records: 0,
        collection_error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn retention_filters_unknown_usage_pagination_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.duckdb");
        let store = Analytics::open(&path).unwrap();
        let now = now_ms();
        let row = AiRequestRecord {
            timestamp: now - 100,
            provider: "p'_%".into(),
            model: "alias".into(),
            outcome: "success".into(),
            duration_ms: 200,
            input_tokens: Some(11),
            output_tokens: Some(7),
            total_tokens: Some(18),
            usage_complete: true,
            ..Default::default()
        };
        {
            let mut db = store.db.lock().unwrap();
            insert(
                &db,
                &AiRequestRecord {
                    timestamp: now - RETENTION_MS - 1,
                    ..row.clone()
                },
            )
            .unwrap();
            insert(
                &db,
                &AiRequestRecord {
                    timestamp: now - RETENTION_MS,
                    ..row.clone()
                },
            )
            .unwrap();
            insert(&db, &row).unwrap();
            insert(
                &db,
                &AiRequestRecord {
                    outcome: "cancelled".into(),
                    duration_ms: 1200,
                    usage_complete: false,
                    ..row.clone()
                },
            )
            .unwrap();
            insert(
                &db,
                &AiRequestRecord {
                    provider: "other".into(),
                    outcome: "http_error".into(),
                    duration_ms: 4000,
                    usage_complete: false,
                    ..row.clone()
                },
            )
            .unwrap();
            Analytics::prune(&mut db, now).unwrap();
            let result = query(&db, now - RETENTION_MS, now, &Filter::default()).unwrap();
            assert_eq!(result.totals.requests, 4);
            assert_eq!(result.totals.succeeded, 2);
            assert_eq!(result.totals.cancelled, 1);
            assert_eq!(result.totals.failed, 1);
            assert_eq!(result.totals.usage_known, 2);
            assert_eq!(result.totals.total_tokens, 36);
            assert_eq!(result.totals.avg_duration_ms, Some(1400.0));
            assert!((result.totals.p95_duration_ms.unwrap() - 3580.0).abs() < 0.001);
            let result = query(
                &db,
                now - 1000,
                now,
                &Filter {
                    provider: "p'_%".into(),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(result.totals.requests, 2);
            assert_eq!(result.totals.input_tokens, 11);
            for _ in 0..102 {
                insert(&db, &row).unwrap();
            }
            let first = query(&db, now - 1000, now, &Filter::default()).unwrap();
            let second = query(
                &db,
                now - 1000,
                now,
                &Filter {
                    before: first.next_before,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(first.records.len(), 100);
            assert_eq!(second.records.len(), 5);
            assert_eq!(second.totals.requests, 105);
            assert!(second
                .records
                .iter()
                .all(|r| r.id < first.next_before.unwrap()));
        }
        drop(store);
        let reopened = Analytics::open(&path).unwrap();
        assert_eq!(
            reopened
                .ai_summary(Filter::default())
                .await
                .unwrap()
                .totals
                .requests,
            105
        );
    }

    #[tokio::test]
    async fn saturated_statistics_never_wait_for_the_database() {
        let store = Arc::new(Analytics::initialize(Connection::open_in_memory().unwrap()).unwrap());
        let permit = store
            .ai_pending
            .clone()
            .acquire_many_owned(MAX_PENDING as u32)
            .await
            .unwrap();
        store.record_ai(AiRequestRecord::default());
        assert_eq!(store.ai_dropped.load(Ordering::Relaxed), 1);
        drop(permit);
        store.record_ai(AiRequestRecord {
            timestamp: now_ms() - 1,
            outcome: "success".into(),
            ..Default::default()
        });
        store.drain_ai().await;
        assert_eq!(
            store
                .ai_summary(Filter::default())
                .await
                .unwrap()
                .totals
                .requests,
            1
        );
    }
}
