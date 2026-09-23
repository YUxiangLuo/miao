use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
};

use crate::{
    analytics::{now_ms, Analytics, LogFilter},
    models::analytics::{AnalyticsLogs, AnalyticsSummary},
    responses::{status_error, success, HandlerResult},
    state::AppState,
};

impl LogFilter {
    fn validate(&self) -> Result<(), &'static str> {
        if !matches!(self.hours.unwrap_or(24), 1 | 6 | 24) {
            return Err("hours must be 1, 6 or 24");
        }
        if !(1..=500).contains(&self.limit.unwrap_or(100)) {
            return Err("limit must be between 1 and 500");
        }
        if self.before.is_some_and(|v| v <= 0) || self.id.is_some_and(|v| v <= 0) {
            return Err("before must be a positive log ID");
        }
        if [&self.q, &self.component, &self.context, &self.event]
            .iter()
            .any(|q| q.len() > 1024)
        {
            return Err("search is limited to 1024 bytes");
        }
        if !matches!(
            self.level.as_str(),
            "" | "trace" | "debug" | "info" | "warn" | "error" | "fatal" | "panic" | "unknown"
        ) {
            return Err("invalid log level");
        }
        Ok(())
    }
}

pub async fn logs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LogFilter>,
) -> HandlerResult<AnalyticsLogs> {
    query
        .validate()
        .map_err(|e| status_error(StatusCode::BAD_REQUEST, e))?;
    let store = state.analytics.get().ok_or_else(|| {
        status_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Network history is not initialized",
        )
    })?;
    let now = now_ms();
    let result = store
        .read(move |db| {
            Analytics::logs(db, now - query.hours.unwrap_or(24) * 3_600_000, now, &query)
        })
        .await;
    result
        .map(|data| success("Network logs", data))
        .map_err(|e| status_error(StatusCode::INTERNAL_SERVER_ERROR, e))
}

pub async fn summary(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LogFilter>,
) -> HandlerResult<Box<AnalyticsSummary>> {
    query
        .validate()
        .map_err(|e| status_error(StatusCode::BAD_REQUEST, e))?;
    let store = state.analytics.get().ok_or_else(|| {
        status_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Network history is not initialized",
        )
    })?;
    let now = now_ms();
    let mut data = store
        .read(move |db| Analytics::summary(db, now - query.hours.unwrap_or(24) * 3_600_000, now))
        .await
        .map_err(|e| status_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let errors: Vec<String> = [
        store.log_error.lock().unwrap().clone(),
        store.sample_error.lock().unwrap().clone(),
    ]
    .into_iter()
    .flatten()
    .collect();
    data.collection_error = (!errors.is_empty()).then(|| errors.join("; "));
    Ok(success("Network analytics", Box::new(data)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn api_validates_bounds_and_reads_history_without_a_running_kernel() {
        let state = Arc::new(AppState::new(crate::models::Config::default()).unwrap());
        let temp = tempfile::tempdir().unwrap();
        let store = Analytics::open(&temp.path().join("history/network.duckdb")).unwrap();
        assert!(state.analytics.set(store).is_ok());
        let app = crate::router::build_router(state);
        for query in [
            "hours=25",
            "hours=0",
            "limit=501",
            "limit=0",
            "before=-1",
            "level=oops",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/analytics/logs?{query}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
        }
        for query in [
            "hours=24&limit=100",
            "id=4&level=info",
            "before=42&component=dns&event=dns.complete&context=run%2F99",
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/analytics/logs?{query}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let body = to_bytes(response.into_body(), 100_000).await.unwrap();
            assert_eq!(
                status,
                StatusCode::OK,
                "{query}: {}",
                String::from_utf8_lossy(&body)
            );
        }
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/analytics/summary?hours=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 100_000).await.unwrap())
                .unwrap();
        assert_eq!(body["data"]["upload"], 0);
        assert_eq!(body["data"]["sampled_connections"], 0);
        assert!(body["data"]["last_sample_at"].is_null());
    }
}
