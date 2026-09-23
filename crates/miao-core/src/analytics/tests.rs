use super::*;

fn log(timestamp: i64, message: &str) -> LogRecord {
    LogRecord {
        timestamp,
        stream: "stderr",
        session: "test-kernel".into(),
        raw: message.as_bytes().to_vec(),
    }
}

#[test]
fn retention_boundaries_pagination_search_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("analytics/history.duckdb");
    let store = Analytics::open(&path).unwrap();
    let now = now_ms();
    let cutoff = now - RETENTION_MS;
    {
        let mut db = store.db.lock().unwrap();
        Analytics::insert_logs(
            &mut db,
            &[
                log(cutoff - 1, "ERROR expired"),
                log(cutoff, "WARN exact boundary"),
                log(now - 5, "INFO literal 50%_quote'"),
                log(now - 4, "ERROR dns failed"),
                log(now - 3, "INFO literal 50%_quote' again"),
            ],
        )
        .unwrap();
        // Queries hide expired records even before the cleanup worker runs.
        let first = Analytics::logs(
            &db,
            cutoff,
            now,
            &LogFilter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            first.entries.iter().map(|r| r.id).collect::<Vec<_>>(),
            [5, 4]
        );
        assert_eq!(first.next_before, Some(4));
        let second = Analytics::logs(
            &db,
            cutoff,
            now,
            &LogFilter {
                before: Some(4),
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            second.entries.iter().map(|r| r.id).collect::<Vec<_>>(),
            [3, 2]
        );
        assert!(second.next_before.is_none());
        let search = Analytics::logs(
            &db,
            cutoff,
            now,
            &LogFilter {
                level: "info".into(),
                q: "50%_quote'".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(search.entries.len(), 2);
        assert_eq!(Analytics::summary(&db, cutoff, now).unwrap().error_count, 1);
        Analytics::prune(&db, now).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM logs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
    }
    drop(store);
    let reopened = Analytics::open(&path).unwrap();
    let mut db = reopened.db.lock().unwrap();
    Analytics::insert_logs(&mut db, &[log(now, "DEBUG after restart")]).unwrap();
    let page = Analytics::logs(&db, now - 1000, now, &LogFilter::default()).unwrap();
    assert_eq!(page.entries.len(), 4);
    assert!(page.entries[0].id > 5);
}

#[test]
fn aggregates_count_distinct_connections_and_sum_only_in_window_deltas() {
    let store = Analytics::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let mut db = store.db.lock().unwrap();
    let since = 900_000;
    let delta = |id: &str, domain: &str, upload, download| ConnectionDelta {
        id: id.into(),
        domain: domain.into(),
        source: "10.0.0.2".into(),
        upload,
        download,
        ..Default::default()
    };
    Analytics::insert_sample(&mut db, since - 1, &[delta("a", "a.test", 999, 999)]).unwrap();
    Analytics::insert_sample(&mut db, since, &[delta("a", "a.test", 10, 80)]).unwrap();
    Analytics::insert_sample(
        &mut db,
        since + 300_001,
        &[delta("a", "a.test", 20, 60), delta("b", "b.test", 200, 5)],
    )
    .unwrap();
    Analytics::insert_logs(
        &mut db,
        &[
            log(since, "ERROR boundary"),
            log(since - 1, "ERROR excluded"),
        ],
    )
    .unwrap();
    let result = Analytics::summary(&db, since, since + 600_000).unwrap();
    assert_eq!(
        (result.upload, result.download, result.sampled_connections),
        (230, 145, 2)
    );
    let groups: Vec<_> = result
        .groups
        .iter()
        .filter(|g| g.dimension == "domain")
        .collect();
    assert_eq!(groups[0].name, "b.test");
    assert_eq!(
        (groups[1].connections, groups[1].upload, groups[1].download),
        (1, 30, 140)
    );
    assert_eq!(
        (
            result.timeline[0].upload,
            result.timeline[0].download,
            result.timeline[0].error_count
        ),
        (10, 80, 1)
    );
    assert_eq!(result.timeline[1].upload, 220);
    assert_eq!(result.timeline[2].upload, 0);
    assert_eq!(result.last_sample_at, Some(since + 300_001));
    Analytics::prune(&db, since + RETENTION_MS + 1).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM traffic", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM samples", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM logs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn capture_flushes_sparse_lines_partial_eof_and_preserves_original_bytes() {
    use tokio::io::AsyncWriteExt;
    let dir = tempfile::tempdir().unwrap();
    let store = Analytics::open(&dir.path().join("analytics/history.duckdb")).unwrap();
    let (mut writer, reader) = tokio::io::duplex(1024);
    let task = capture(reader, "stdout", store.clone(), "test-kernel".into());
    writer.write_all(b"INFO first\nDEBUG part").await.unwrap();
    // A quiet, still-open stream must flush completed lines, not await EOF.
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let count = store
                .run(|db| db.query_row("SELECT count(*) FROM logs", [], |r| r.get::<_, i64>(0)))
                .await
                .unwrap();
            if count == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    writer.write_all(b"ial\xff").await.unwrap();
    drop(writer);
    task.await.unwrap();
    let raw = store
        .run(|db| {
            db.prepare("SELECT raw FROM logs ORDER BY id")?
                .query_map([], |r| r.get::<_, Vec<u8>>(0))?
                .collect::<duckdb::Result<Vec<_>>>()
        })
        .await
        .unwrap();
    assert_eq!(
        raw,
        [b"INFO first\n".to_vec(), b"DEBUG partial\xff".to_vec()]
    );
}

#[test]
fn severity_comes_from_prefix_not_message_body() {
    assert_eq!(
        log_level("+0900 2026-09-23 16:42:30 ERROR [123 45ms] dns: failed"),
        "error"
    );
    assert_eq!(
        log_level("-0700 2026-09-23 16:42:30 WARN [123 45ms] dns: failed"),
        "warn"
    );
    assert_eq!(
        log_level("\x1b[36mINFO\x1b[0m[0012] contains ERROR text"),
        "info"
    );
    assert_eq!(log_level("panic: unformatted runtime failure"), "unknown");
    assert_eq!(log_level("some unsupported ERROR format"), "unknown");
}

#[test]
fn dns_events_have_real_latency_quantiles_exact_drilldown_and_retention() {
    let store = Analytics::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let mut db = store.db.lock().unwrap();
    let now = now_ms();
    let event = |domain: &str, duration_ms, outcome: &str| {
        format!(
            "INFO [99 9m] dns: MIAO_DNS {}",
            serde_json::json!({
                "v": 1, "domain": domain, "query_type": "A", "duration_ms": duration_ms,
                "outcome": outcome, "rcode": if outcome == "success" {Some(0)} else {Some(3)}, "error": null, "transport": "dns-local"
            })
        )
    };
    Analytics::insert_logs(
        &mut db,
        &[
            log(
                now - RETENTION_MS - 1,
                &event("expired.test", 9999.0, "error"),
            ),
            log(now - 4, &event("fast.test", 0.25, "success")),
            log(now - 3, &event("slow.test", 200.0, "dns_error")),
            log(now - 2, &event("fast.test", 10.0, "success")),
            log(now - 1, "DEBUG [99 10m] dns: cached fast.test NOERROR 50"),
            log(now, "INFO dns: MIAO_DNS {broken"),
        ],
    )
    .unwrap();
    let summary = Analytics::summary(&db, now - RETENTION_MS, now).unwrap();
    assert_eq!(summary.dns.queries, 3);
    assert_eq!(summary.dns.failures, 1);
    assert_eq!(summary.dns.p50_ms, Some(10.0));
    assert!((summary.dns.p95_ms.unwrap() - 181.0).abs() < 0.0001);
    assert_eq!(summary.dns.slowest[0].domain, "slow.test");
    assert_eq!(summary.dns.slowest[0].duration_ms, 200.0);
    assert_eq!(summary.dns.slowest[0].log_id, 3);
    let raw = Analytics::logs(
        &db,
        now - RETENTION_MS,
        now,
        &LogFilter {
            id: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(raw.entries.len(), 1);
    assert!(raw.entries[0].message.contains("slow.test"));
    let context = Analytics::logs(
        &db,
        now - RETENTION_MS,
        now,
        &LogFilter {
            context: "test-kernel/99".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(context.entries.len(), 4);
    let completed = Analytics::logs(
        &db,
        now - RETENTION_MS,
        now,
        &LogFilter {
            event: "dns.complete".into(),
            component: "dns".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(completed.entries.len(), 3);
    Analytics::prune(&db, now).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM dns_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}
