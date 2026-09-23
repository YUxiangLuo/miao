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
        Analytics::prune(&mut db, now).unwrap();
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
    Analytics::prune(&mut db, since + RETENTION_MS + 1).unwrap();
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
    Analytics::prune(&mut db, now).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM dns_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn issue_groups_separate_severity_and_coverage_and_keep_quiet_outbounds() {
    let store = Analytics::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let mut db = store.db.lock().unwrap();
    let now = now_ms();
    let mut rows = vec![
        log(now - 4, "ERROR [7 1ms] dns: exchange failed: timeout"),
        log(now - 3, "ERROR [7 2ms] dns: exchange failed: timeout"),
        log(now - 2, "WARN [8 1ms] dns: slow upstream"),
        log(now - 1, "INFO [9 1ms] dns: message containing ERROR"),
        log(now, "runtime stack continuation"),
        log(
            now,
            "INFO [10 1ms] outbound/direct[quiet]: outbound connection to a.test:443",
        ),
        log(
            now,
            "INFO [10 1ms] inbound/tun[tun]: inbound connection from 10.0.0.2:1234",
        ),
        log(now - RETENTION_MS - 1, "FATAL dns: expired"),
    ];
    for i in 0..21 {
        for _ in 0..2 {
            rows.push(log(now, &format!("DEBUG noisy{i}: background activity")));
        }
    }
    Analytics::insert_logs(&mut db, &rows).unwrap();
    let summary = Analytics::summary(&db, now - RETENTION_MS, now).unwrap();
    assert_eq!(summary.log_count, 49);
    assert_eq!(summary.error_count, 2);
    assert_eq!(summary.activity.parsed_count, 48);
    assert_eq!(summary.activity.classified_count, 2);
    assert_eq!(summary.activity.issues.len(), 2);
    let errors = &summary.activity.issues[0];
    assert_eq!(
        (errors.component.as_str(), errors.level.as_str()),
        ("dns", "error")
    );
    assert_eq!(
        (errors.count, errors.contexts, errors.latest_at),
        (2, 1, now - 3)
    );
    assert_eq!(summary.activity.issues[1].level, "warn");
    assert!(!summary
        .activity
        .facets
        .iter()
        .any(|f| f.dimension == "component" && f.name == "outbound/direct[quiet]"));
    assert!(summary
        .activity
        .facets
        .iter()
        .any(|f| f.dimension == "outbound" && f.name == "outbound/direct[quiet]" && f.count == 1));
    assert!(summary
        .activity
        .facets
        .iter()
        .any(|f| f.dimension == "inbound" && f.name == "inbound/tun[tun]" && f.count == 1));
}

#[test]
fn dns_rankings_include_rare_slow_and_failed_domains_outside_popular_top_twenty() {
    let store = Analytics::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let mut db = store.db.lock().unwrap();
    let now = now_ms();
    let event = |domain: &str, duration: f64, outcome: &str| {
        log(
            now,
            &format!(
                "INFO dns: MIAO_DNS {}",
                serde_json::json!({"v":1,"domain":domain,"query_type":"A","duration_ms":duration,"outcome":outcome})
            ),
        )
    };
    let mut rows = Vec::new();
    for i in 0..21 {
        for _ in 0..3 {
            rows.push(event(&format!("popular-{i}.test"), 2.0, "success"));
        }
    }
    rows.push(event("rare-slow.test", 1234.5, "success"));
    rows.push(event("rare-failure.test", 400.0, "dns_error"));
    rows.push(event("rare-failure.test", 10.0, "success"));
    Analytics::insert_logs(&mut db, &rows).unwrap();
    let dns = Analytics::summary(&db, now - RETENTION_MS, now)
        .unwrap()
        .dns;
    assert!(!dns
        .groups
        .iter()
        .any(|g| g.dimension == "domain" && g.name.starts_with("rare-")));
    assert_eq!(dns.slow_domains[0].name, "rare-slow.test");
    assert_eq!(dns.slow_domains[0].max_ms, 1234.5);
    assert_eq!(dns.failed_domains.len(), 1);
    let failed = &dns.failed_domains[0];
    assert_eq!(failed.name, "rare-failure.test");
    assert_eq!((failed.queries, failed.failures), (2, 1));
    assert!((failed.p95_ms - 380.5).abs() < 0.0001);
}

#[test]
fn timeline_starts_at_first_record_in_window_including_idle_samples() {
    let store = Analytics::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let mut db = store.db.lock().unwrap();
    let since = 900_000;
    let until = 2_100_000;
    Analytics::insert_logs(
        &mut db,
        &[log(since - 1, "INFO old"), log(until + 1, "INFO future")],
    )
    .unwrap();
    assert!(Analytics::summary(&db, since, until)
        .unwrap()
        .timeline
        .is_empty());

    Analytics::insert_sample(&mut db, 1_500_001, &[]).unwrap();
    let idle = Analytics::summary(&db, since, until).unwrap();
    assert_eq!((idle.since, idle.until), (since, until));
    assert_eq!(
        idle.timeline
            .iter()
            .map(|b| b.timestamp)
            .collect::<Vec<_>>(),
        [1_500_000, 1_800_000, 2_100_000]
    );
    assert!(idle
        .timeline
        .iter()
        .all(|b| b.upload == 0 && b.download == 0 && b.log_count == 0));

    Analytics::insert_logs(
        &mut db,
        &[log(1_200_001, "INFO first"), log(1_800_010, "ERROR later")],
    )
    .unwrap();
    let recorded = Analytics::summary(&db, since, until).unwrap();
    assert_eq!(
        recorded
            .timeline
            .iter()
            .map(|b| (b.timestamp, b.log_count))
            .collect::<Vec<_>>(),
        [
            (1_200_000, 1),
            (1_500_000, 0),
            (1_800_000, 1),
            (2_100_000, 0)
        ]
    );
    assert_eq!(recorded.timeline[2].error_count, 1);
}

fn dns_log(timestamp: i64, context: u32, domain: &str) -> LogRecord {
    log(
        timestamp,
        &format!(
            "INFO [{context} 2ms] dns: MIAO_DNS {}",
            serde_json::json!({
                "v": 1, "domain": domain, "query_type": "AAAA", "duration_ms": 0.125,
                "outcome": "success", "rcode": 0, "error": null, "transport": "dns-local"
            })
        ),
    )
}

#[test]
fn log_batch_failure_rolls_back_both_tables_and_same_batch_can_retry() {
    let store = Analytics::initialize(Connection::open_in_memory().unwrap()).unwrap();
    let mut db = store.db.lock().unwrap();
    let now = now_ms();
    Analytics::insert_logs(&mut db, &[dns_log(now, 7, "existing.test")]).unwrap();
    // Appender constraint errors can surface only during flush. Force a failure
    // in the second table after logs have been flushed into the transaction.
    db.execute_batch("CREATE UNIQUE INDEX test_dns_context ON dns_events(context)")
        .unwrap();
    let rows = vec![
        log(now + 1, "ERROR [42 1ms] router: first in failed batch"),
        dns_log(now + 2, 8, "first.test"),
        dns_log(now + 3, 7, "conflicting.test"),
    ];
    assert!(Analytics::insert_logs(&mut db, &rows).is_err());
    for table in ["logs", "dns_events"] {
        let count: i64 = db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "partial batch persisted in {table}");
    }
    db.execute_batch("DROP INDEX test_dns_context").unwrap();
    Analytics::insert_logs(&mut db, &rows).unwrap();
    let raw = db
        .prepare("SELECT raw FROM logs ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get::<_, Vec<u8>>(0))
        .unwrap()
        .collect::<duckdb::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(
        &raw[1..],
        rows.iter().map(|r| r.raw.clone()).collect::<Vec<_>>()
    );
    let domains = db
        .prepare("SELECT domain FROM dns_events ORDER BY log_id")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<duckdb::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(domains, ["existing.test", "first.test", "conflicting.test"]);
    let (distinct, linked): (i64, i64) = db.query_row(
        "SELECT (SELECT count(DISTINCT id) FROM logs), (SELECT count(*) FROM dns_events d JOIN logs l ON l.id = d.log_id AND l.context = d.context)",
        [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((distinct, linked), (4, 3));
}

#[test]
fn large_mixed_log_batches_keep_bytes_order_and_exact_dns_links_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history/data.duckdb");
    let now = now_ms();
    // Cross the appender's vector boundary as well as the collector's 256 rows.
    let rows: Vec<_> = (0..4097)
        .map(|i| {
            if i % 3 == 1 {
                dns_log(now + i, i as u32, &format!("域名-{i}.test"))
            } else {
                let mut row = log(
                    now + i,
                    &format!("ERROR [{i} 3ms] router: row {i}\0quoted'"),
                );
                row.raw.push(0xff);
                row.raw.push(b'\n');
                row.stream = "stdout";
                row.session = "second-kernel".into();
                row
            }
        })
        .collect();
    {
        let store = Analytics::open(&path).unwrap();
        let mut db = store.db.lock().unwrap();
        // Retain compatibility with databases already written by the SQL path.
        db.execute_batch("INSERT INTO logs (timestamp,stream,level,message,raw,component,event,context,route) VALUES (0,'stderr','info','old','old'::BLOB,'core','other','','')").unwrap();
        Analytics::insert_logs(&mut db, &[]).unwrap();
        for batch in [&rows[..257], &rows[257..]] {
            Analytics::insert_logs(&mut db, batch).unwrap();
        }
    }
    let reopened = Analytics::open(&path).unwrap();
    let db = reopened.db.lock().unwrap();
    let actual = db
        .prepare("SELECT id, timestamp, stream, raw, message, context FROM logs ORDER BY id")
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Vec<u8>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .unwrap()
        .collect::<duckdb::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(actual.len(), rows.len());
    for (i, (got, expected)) in actual.iter().zip(&rows).enumerate() {
        assert_eq!(got.0, i as i64 + 2);
        assert_eq!(got.1, expected.timestamp);
        assert_eq!(got.2, expected.stream);
        assert_eq!(got.3, expected.raw);
        assert_eq!(got.4, String::from_utf8_lossy(&expected.raw));
        assert_eq!(got.5, format!("{}/{i}", expected.session));
    }
    let dns = db.prepare("SELECT d.log_id, d.domain, d.duration_ms, d.rcode, d.error, d.transport FROM dns_events d JOIN logs l ON l.id = d.log_id AND l.context = d.context ORDER BY d.log_id").unwrap()
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, Option<i32>>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?)))
        .unwrap().collect::<duckdb::Result<Vec<_>>>().unwrap();
    assert_eq!(dns.len(), (1..4097).step_by(3).count());
    for (got, i) in dns.iter().zip((1..4097).step_by(3)) {
        assert_eq!(
            got,
            &(
                i as i64 + 2,
                format!("域名-{i}.test"),
                0.125,
                Some(0),
                None,
                Some("dns-local".into())
            )
        );
    }
}

// Paired benchmark reference: the pre-Appender implementation. Keep parsing,
// sequence assignment, transaction boundaries and DNS indexing in both paths.
fn insert_logs_rowwise(db: &mut Connection, rows: &[LogRecord]) -> duckdb::Result<()> {
    let tx = db.transaction()?;
    {
        let mut insert = tx.prepare("INSERT INTO logs (timestamp, stream, level, message, raw, component, event, context, route) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id")?;
        let mut dns_insert =
            tx.prepare("INSERT INTO dns_events VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")?;
        for row in rows {
            let message = String::from_utf8_lossy(&row.raw);
            let parsed = events::parse(&message, &row.session);
            let id: i64 = insert.query_row(
                params![
                    row.timestamp,
                    row.stream,
                    log_level(&message),
                    message.as_ref(),
                    row.raw,
                    parsed.component,
                    parsed.event,
                    parsed.context,
                    parsed.route
                ],
                |r| r.get(0),
            )?;
            if let Some(dns) = parsed.dns {
                dns_insert.execute(params![
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
    }
    tx.commit()
}

#[test]
#[ignore = "file-backed paired performance benchmark; run alone with --ignored --nocapture"]
fn benchmark_log_ingestion() {
    use std::time::Instant;
    for (label, size, dns_every) in [
        ("sparse", 1, 0),
        ("text", 256, 0),
        ("mixed", 256, 4),
        ("dns", 256, 1),
    ] {
        let rows: Vec<_> = (0..size).map(|i| {
            if dns_every > 0 && i % dns_every == 0 {
                dns_log(now_ms(), i as u32, &format!("domain-{i}.test"))
            } else {
                log(now_ms(), &format!("ERROR [{i} 4ms] outbound/direct[direct]: connection to host-{i}.test:443 failed: timeout\n"))
            }
        }).collect();
        let mut timings = [Vec::new(), Vec::new()];
        for round in 0..6 {
            for variant in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let dir = tempfile::tempdir().unwrap();
                let store = Analytics::open(&dir.path().join("history/data.duckdb")).unwrap();
                let mut db = store.db.lock().unwrap();
                let insert = if variant == 0 {
                    insert_logs_rowwise
                } else {
                    Analytics::insert_logs
                };
                insert(&mut db, &rows).unwrap(); // Warm parser and DB outside measurement.
                let start = Instant::now();
                for _ in 0..10 {
                    insert(&mut db, &rows).unwrap();
                }
                timings[variant].push(start.elapsed().as_secs_f64() * 1000.0 / 10.0);
                let (logs, distinct, dns): (i64, i64, i64) = db.query_row("SELECT (SELECT count(*) FROM logs), (SELECT count(DISTINCT id) FROM logs), (SELECT count(*) FROM dns_events)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
                let expected_dns = if dns_every == 0 {
                    0
                } else {
                    (0..size).filter(|i| i % dns_every == 0).count()
                };
                assert_eq!(
                    (logs, distinct, dns),
                    (
                        (size * 11) as i64,
                        (size * 11) as i64,
                        (expected_dns * 11) as i64
                    )
                );
            }
        }
        for (variant, mut values) in timings.into_iter().enumerate() {
            values.sort_by(f64::total_cmp);
            eprintln!(
                "{label}: variant={variant} rows={size} median_ms={:.3} samples={values:?}",
                (values[2] + values[3]) / 2.0
            );
        }
    }
}

#[tokio::test]
async fn reader_snapshot_allows_writes_retention_and_checkpoint() {
    use std::time::Duration;
    const IDS: &str = "SELECT
        (SELECT string_agg(id::VARCHAR, ',' ORDER BY id) FROM logs),
        (SELECT string_agg(log_id::VARCHAR, ',' ORDER BY log_id) FROM dns_events)";
    let dir = tempfile::tempdir().unwrap();
    let store = Analytics::open(&dir.path().join("history/data.duckdb")).unwrap();
    let now = now_ms();
    store
        .run(move |db| {
            Analytics::insert_logs(
                db,
                &[
                    dns_log(now - RETENTION_MS - 1, 1, "expired.test"),
                    dns_log(now, 2, "retained.test"),
                ],
            )
        })
        .await
        .unwrap();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let reader_store = store.clone();
    let reader = tokio::spawn(async move {
        reader_store
            .read(move |db| {
                let before: (String, String) =
                    db.query_row(IDS, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
                started.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(10)).unwrap();
                let after: (String, String) =
                    db.query_row(IDS, [], |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok((before, after))
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), ready)
        .await
        .unwrap()
        .unwrap();
    // A single connection mutex would deadlock this write until reader release.
    // Exercise real DELETE + CHECKPOINT, not merely a mocked writer callback.
    let written = tokio::time::timeout(
        Duration::from_secs(5),
        store.run(move |db| {
            Analytics::insert_logs(db, &[dns_log(now, 3, "new.test")])?;
            Analytics::prune(db, now)
        }),
    )
    .await;
    release.send(()).unwrap();
    let snapshot = reader.await.unwrap().unwrap();
    written.expect("reader blocked ingestion/pruning").unwrap();
    assert_eq!(
        snapshot,
        (("1,2".into(), "1,2".into()), ("1,2".into(), "1,2".into()))
    );
    let fresh: (String, String) = store
        .read(|db| db.query_row(IDS, [], |r| Ok((r.get(0)?, r.get(1)?))))
        .await
        .unwrap();
    assert_eq!(fresh, ("2,3".into(), "2,3".into()));
    // A failed query must roll back its read transaction and leave the reusable
    // connection usable; it must not affect the separate writer transaction.
    assert!(store
        .read(|db| db.execute_batch("SELECT * FROM missing_table"))
        .await
        .is_err());
    let fresh: (String, String) = store
        .read(|db| db.query_row(IDS, [], |r| Ok((r.get(0)?, r.get(1)?))))
        .await
        .unwrap();
    assert_eq!(fresh, ("2,3".into(), "2,3".into()));
}

#[tokio::test]
#[ignore = "file-backed query/write contention benchmark; run alone in release mode"]
async fn benchmark_writes_during_summary() {
    use std::time::Instant;
    let mut timings = [Vec::new(), Vec::new()];
    for round in 0..6 {
        for variant in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
            let dir = tempfile::tempdir().unwrap();
            let store = Analytics::open(&dir.path().join("history/data.duckdb")).unwrap();
            let now = now_ms();
            store.run(move |db| {
                for batch in 0..64 {
                    let rows: Vec<_> = (0..256).map(|i| {
                        let index = batch * 256 + i;
                        if i % 4 == 0 {
                            dns_log(now - 1000, index, &format!("host-{}.test", index % 512))
                        } else {
                            log(now - 1000, &format!("ERROR [{index} 7ms] outbound/direct[direct]: connection failed: timeout"))
                        }
                    }).collect();
                    Analytics::insert_logs(db, &rows)?;
                }
                Ok(())
            }).await.unwrap();
            let reader_store = store.clone();
            let (started, ready) = tokio::sync::oneshot::channel();
            let read = tokio::spawn(async move {
                let work = move |db: &Connection| {
                    // Establish the snapshot before allowing the write to start.
                    let count: i64 = db.query_row("SELECT count(*) FROM logs", [], |r| r.get(0))?;
                    assert_eq!(count, 16384);
                    started.send(()).unwrap();
                    Analytics::summary(db, now - RETENTION_MS, now)
                };
                if variant == 0 {
                    reader_store.run(move |db| work(db)).await
                } else {
                    reader_store.read(work).await
                }
            });
            ready.await.unwrap();
            let rows: Vec<_> = (0..256)
                .map(|i| dns_log(now - 1, 900_000 + i, "new.test"))
                .collect();
            let start = Instant::now();
            store
                .run(move |db| Analytics::insert_logs(db, &rows))
                .await
                .unwrap();
            timings[variant].push(start.elapsed().as_secs_f64() * 1000.0);
            let summary = read.await.unwrap().unwrap();
            assert_eq!((summary.log_count, summary.dns.queries), (16384, 4096));
            let counts: (i64, i64) = store
                .read(|db| {
                    db.query_row(
                        "SELECT (SELECT count(*) FROM logs), (SELECT count(*) FROM dns_events)",
                        [],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                })
                .await
                .unwrap();
            assert_eq!(counts, (16640, 4352));
        }
    }
    for (variant, mut values) in timings.into_iter().enumerate() {
        values.sort_by(f64::total_cmp);
        eprintln!(
            "write_during_summary: variant={variant} median_ms={:.3} samples={values:?}",
            (values[2] + values[3]) / 2.0
        );
    }
}
