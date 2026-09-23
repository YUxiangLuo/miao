use std::{collections::HashMap, sync::Arc, time::Duration};

use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    task::JoinHandle,
};

use super::{now_ms, Analytics, ConnectionDelta, LogRecord};
use crate::{services::singbox::CLASH_API_BASE, state::AppState};

/// A bounded batch applies backpressure rather than silently dropping logs.
/// read_until is cancellation-safe and preserves even a final unterminated line.
pub fn capture(
    reader: impl AsyncRead + Unpin + Send + 'static,
    stream: &'static str,
    store: Arc<Analytics>,
    session: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        let mut batch = Vec::new();
        let mut bytes = 0;
        let mut timer = tokio::time::interval(Duration::from_millis(250));
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let mut eof = false;
            tokio::select! {
                result = reader.read_until(b'\n', &mut line) => match result {
                    Ok(0) => eof = true,
                    Ok(_) => {
                        bytes += line.len();
                        batch.push(LogRecord {timestamp: now_ms(), stream, session: session.clone(), raw: std::mem::take(&mut line)});
                        if batch.len() < 256 && bytes < 1024 * 1024 { continue; }
                    }
                    Err(error) => {
                        *store.log_error.lock().unwrap() = Some(format!("{stream} capture failed: {error}"));
                        tracing::error!(%error, stream, "sing-box log capture failed");
                        if !line.is_empty() {
                            batch.push(LogRecord {timestamp: now_ms(), stream, session: session.clone(), raw: std::mem::take(&mut line)});
                        }
                        eof = true;
                    }
                },
                _ = timer.tick() => {}
            }
            if !batch.is_empty() {
                let rows = Arc::new(std::mem::take(&mut batch));
                loop {
                    let pending = rows.clone();
                    match store
                        .run(move |db| Analytics::insert_logs(db, &pending))
                        .await
                    {
                        Ok(()) => break,
                        Err(error) => {
                            // Sticky health warning: a gap/disk problem must not
                            // be hidden by the next successful sample/write.
                            *store.log_error.lock().unwrap() =
                                Some(format!("Log persistence delayed: {error}"));
                            tracing::error!(%error, "Retrying sing-box log batch");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
                bytes = 0;
            }
            if eof {
                break;
            }
        }
    })
}

#[derive(Deserialize)]
struct Snapshot {
    connections: Vec<ClashConnection>,
}

#[derive(Deserialize)]
struct ClashConnection {
    id: String,
    upload: i64,
    download: i64,
    #[serde(default)]
    chains: Vec<String>,
    #[serde(default)]
    rule: String,
    #[serde(default, rename = "rulePayload")]
    rule_payload: String,
    metadata: Metadata,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Metadata {
    host: String,
    sniff_host: String,
    #[serde(rename = "destinationIP")]
    destination_ip: String,
    #[serde(rename = "sourceIP")]
    source_ip: String,
    process_path: String,
    network: String,
}

fn deltas(
    snapshot: &Snapshot,
    previous: &HashMap<String, (i64, i64)>,
    session: &str,
) -> Vec<ConnectionDelta> {
    snapshot
        .connections
        .iter()
        .map(|connection| {
            let (up, down) = previous.get(&connection.id).copied().unwrap_or_default();
            // A reset is a new counter epoch, never a negative traffic spike.
            let difference = |current: i64, old: i64| {
                if current >= old {
                    current - old
                } else {
                    current.max(0)
                }
            };
            let meta = &connection.metadata;
            ConnectionDelta {
                id: format!("{session}/{}", connection.id),
                domain: [&meta.host, &meta.sniff_host, &meta.destination_ip]
                    .into_iter()
                    .find(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_default(),
                source: meta.source_ip.clone(),
                process: meta.process_path.clone(),
                outbound: connection.chains.first().cloned().unwrap_or_default(),
                rule: if connection.rule_payload.is_empty() {
                    connection.rule.clone()
                } else {
                    format!("{}: {}", connection.rule, connection.rule_payload)
                },
                network: meta.network.clone(),
                upload: difference(connection.upload, up),
                download: difference(connection.download, down),
            }
        })
        .collect()
}

pub fn spawn_sampler(state: Arc<AppState>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let store = state
            .analytics
            .get()
            .expect("analytics initialized before sampling")
            .clone();
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut previous = HashMap::new();
        let mut kernel = None;
        let mut session = String::new();
        let mut last_prune = 0;
        loop {
            tokio::select! {
                _ = tick.tick() => {},
                _ = state.scheduled_refresh_wake.notified() => {},
            }
            if state.lifecycle.is_shutting_down() {
                break;
            }
            let now = now_ms();
            if now - last_prune >= 60_000 {
                if let Err(error) = store.run(move |db| Analytics::prune(db, now)).await {
                    *store.log_error.lock().unwrap() =
                        Some(format!("History cleanup failed: {error}"));
                }
                last_prune = now;
            }
            // Never contact the machine's production Clash API from an idle
            // test/profile. A ready, owned child is required.
            if !state.lifecycle.snapshot().ready {
                continue;
            }
            let identity = state
                .sing_process
                .lock()
                .await
                .as_ref()
                .map(|p| (p.child.id(), p.started_at));
            let Some(identity) = identity else {
                continue;
            };
            if kernel != Some(identity) {
                previous.clear();
                session = format!(
                    "{}-{:?}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(now),
                    identity.0
                );
                kernel = Some(identity);
            }
            let result = async {
                let snapshot = state
                    .http_client
                    .get(format!("{CLASH_API_BASE}/connections"))
                    .timeout(Duration::from_secs(2))
                    .send()
                    .await
                    .map_err(|e| e.to_string())?
                    .error_for_status()
                    .map_err(|e| e.to_string())?
                    .json::<Snapshot>()
                    .await
                    .map_err(|e| e.to_string())?;
                if !state.lifecycle.snapshot().ready
                    || state
                        .sing_process
                        .lock()
                        .await
                        .as_ref()
                        .map(|p| (p.child.id(), p.started_at))
                        != Some(identity)
                {
                    return Ok(());
                }
                let rows = deltas(&snapshot, &previous, &session);
                let timestamp = now_ms();
                store
                    .run(move |db| Analytics::insert_sample(db, timestamp, &rows))
                    .await?;
                previous = snapshot
                    .connections
                    .into_iter()
                    .map(|c| (c.id, (c.upload, c.download)))
                    .collect();
                Ok::<_, String>(())
            }
            .await;
            *store.sample_error.lock().unwrap() = result.err();
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_are_deltas_not_repeated_totals_and_resets_are_nonnegative() {
        let snapshot: Snapshot = serde_json::from_value(serde_json::json!({"connections": [
            {"id":"a","upload":170,"download":11,"chains":["JP","PROXY"],"metadata":{"host":"a.test","sourceIP":"10.0.0.2","network":"tcp"}},
            {"id":"b","upload":9,"download":220,"metadata":{"destinationIP":"1.2.3.4"}}
        ]})).unwrap();
        let previous = HashMap::from([("a".into(), (100, 90))]);
        let rows = deltas(&snapshot, &previous, "kernel-1");
        assert_eq!((rows[0].upload, rows[0].download), (70, 11));
        assert_eq!((rows[1].upload, rows[1].download), (9, 220));
        assert_eq!(rows[0].outbound, "JP");
        assert_eq!(rows[0].source, "10.0.0.2");
        assert_eq!(rows[1].domain, "1.2.3.4");
        assert_ne!(rows[0].id, deltas(&snapshot, &previous, "kernel-2")[0].id);
    }
}
