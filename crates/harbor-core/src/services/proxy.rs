use std::path::Path;
use std::sync::{atomic::Ordering, Arc};
use std::time::Instant;

use serde_json::Value as JsonValue;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{LastProxy, SwitchProxyResult};
use crate::state::AppState;

const SELECTOR_GROUP: &str = "proxy";
const SWITCH_TIMEOUT: Duration = Duration::from_secs(2);
const RESTORE_POLL_WINDOW: Duration = Duration::from_secs(2);
const RESTORE_GET_TIMEOUT: Duration = Duration::from_millis(300);
const RESTORE_POLL_INTERVAL: Duration = Duration::from_millis(50);
const RESTORE_PUT_TIMEOUT: Duration = Duration::from_secs(1);

/// Typed view over a Clash `/proxies/<group>` JSON payload without rejecting
/// auxiliary fields returned by sing-box's Clash API controller.
struct ClashGroupView<'a>(&'a JsonValue);

impl ClashGroupView<'_> {
    fn is_selector(&self) -> bool {
        self.0.get("type").and_then(JsonValue::as_str) == Some("Selector")
    }

    fn has_members_array(&self) -> bool {
        self.0.get("all").and_then(JsonValue::as_array).is_some()
    }

    fn contains_member(&self, name: &str) -> bool {
        self.0
            .get("all")
            .and_then(JsonValue::as_array)
            .is_some_and(|nodes| nodes.iter().any(|node| node.as_str() == Some(name)))
    }

    fn is_selected(&self, name: &str) -> bool {
        self.0.get("now").and_then(JsonValue::as_str) == Some(name)
    }
}

/// Snapshot of both retirement counters captured when a restore task is spawned:
/// - `lifecycle_generation`: retires restore if the kernel restarts/reloads/stops.
/// - `selection_generation`: retires restore if the user switches or saves a
///   proxy within the same kernel generation.
#[derive(Clone, Copy)]
struct RestoreTicket {
    lifecycle_generation: u64,
    selection_generation: u64,
}

impl RestoreTicket {
    fn capture(state: &AppState) -> Self {
        Self {
            lifecycle_generation: state.lifecycle.snapshot().generation,
            selection_generation: state.proxy_selection_generation.load(Ordering::Relaxed),
        }
    }

    fn selection_superseded(self, state: &AppState) -> bool {
        state.proxy_selection_generation.load(Ordering::Relaxed) != self.selection_generation
    }

    fn lifecycle_superseded(self, state: &AppState) -> bool {
        state.lifecycle.snapshot().generation != self.lifecycle_generation
    }
}

fn retire_pending_restores(state: &AppState) {
    state
        .proxy_selection_generation
        .fetch_add(1, Ordering::Relaxed);
}

fn clash_group_url(base: &str, group: &str) -> String {
    format!("{base}/proxies/{}", urlencoding::encode(group))
}

async fn write_last_proxy_file(path: &Path, proxy: &LastProxy) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::context("Failed to create last-proxy directory", e))?;
    }

    let json = serde_json::to_string(proxy)?;
    crate::services::config::write_file_atomic(path, json.as_bytes()).await?;
    Ok(())
}

pub async fn save_last_proxy(state: &AppState, proxy: &LastProxy) -> AppResult<()> {
    let _guard = state.config_update.lock().await;
    retire_pending_restores(state);
    write_last_proxy_file(&state.runtime_paths.last_proxy, proxy).await
}

pub async fn switch_proxy(state: &AppState, proxy: &LastProxy) -> AppResult<SwitchProxyResult> {
    switch_proxy_at(state, proxy, crate::services::singbox::CLASH_API_BASE).await
}

async fn switch_proxy_at(
    state: &AppState,
    proxy: &LastProxy,
    base: &str,
) -> AppResult<SwitchProxyResult> {
    let _guard = state.config_update.lock().await;
    // Explicit user switches require requested `node_select` to be manual;
    // regional `fastest_*` modes must be switched to manual first.
    if proxy.group != SELECTOR_GROUP || !state.node_select_preference.read().await.is_manual() {
        return Err(AppError::message(
            "当前是地区最快模式或无效分组，请先切换为手动选择模式",
        ));
    }
    if !state.lifecycle.snapshot().ready {
        return Err(AppError::message("服务未运行或代理数据面尚未就绪"));
    }

    let url = clash_group_url(base, &proxy.group);
    let payload: JsonValue = state
        .http_client
        .get(&url)
        .timeout(SWITCH_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let group = ClashGroupView(&payload);
    if !group.is_selector() || !group.contains_member(&proxy.name) {
        return Err(AppError::message("节点不在当前手动选择列表中"));
    }

    let changed = !group.is_selected(&proxy.name);
    if changed {
        state
            .http_client
            .put(&url)
            .timeout(SWITCH_TIMEOUT)
            .json(&serde_json::json!({ "name": proxy.name }))
            .send()
            .await?
            .error_for_status()?;
    }

    // Also supersede old restore work when persistence fails: an accepted
    // runtime selection must not be replaced by a previously stored choice.
    retire_pending_restores(state);
    let persisted = write_last_proxy_file(&state.runtime_paths.last_proxy, proxy)
        .await
        .is_ok();
    Ok(SwitchProxyResult { changed, persisted })
}

async fn load_last_proxy(path: &Path) -> Option<LastProxy> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&content).ok()
}

/// 内核启动或配置重载后调用：捕获当前 lifecycle generation 并派生恢复任务。
/// 连续两次运行配置激活会各派生一个任务；旧任务在落地前发现自己监护的代次
/// 已被取代即放弃，避免陈旧的 PUT 覆盖更新那次启动的选择。
pub fn spawn_restore_last_proxy(state: &Arc<AppState>) {
    let Some(mutation) = state.begin_mutation() else {
        return;
    };
    let ticket = RestoreTicket::capture(state);
    let state = state.clone();
    tokio::spawn(async move {
        let _mutation = mutation;
        restore_last_proxy_with_ticket(&state, ticket).await;
    });
}

#[cfg(test)]
async fn restore_last_proxy(state: &Arc<AppState>, generation: u64) {
    let selection = state.proxy_selection_generation.load(Ordering::Relaxed);
    restore_last_proxy_if_current(state, generation, selection).await;
}

#[cfg(test)]
async fn restore_last_proxy_if_current(state: &Arc<AppState>, generation: u64, selection: u64) {
    restore_last_proxy_with_ticket(
        state,
        RestoreTicket {
            lifecycle_generation: generation,
            selection_generation: selection,
        },
    )
    .await;
}

async fn poll_clash_group_for_restore(
    state: &AppState,
    url: &str,
    ticket: RestoreTicket,
) -> Option<JsonValue> {
    let deadline = Instant::now() + RESTORE_POLL_WINDOW;
    loop {
        if ticket.lifecycle_superseded(state) {
            info!("Skipping last-proxy restore: superseded by a newer sing-box start");
            return None;
        }
        match state
            .http_client
            .get(url)
            .timeout(RESTORE_GET_TIMEOUT)
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => return res.json::<JsonValue>().await.ok(),
            _ if Instant::now() < deadline => sleep(RESTORE_POLL_INTERVAL).await,
            _ => return None,
        }
    }
}

async fn restore_last_proxy_with_ticket(state: &Arc<AppState>, ticket: RestoreTicket) {
    let _guard = state.config_update.lock().await;
    if ticket.selection_superseded(state) || ticket.lifecycle_superseded(state) {
        return;
    }

    // Check effective `config.node_select` so temporary regional fallback to
    // `manual` still restores the last manual proxy.
    if !state.config.read().await.node_select.is_manual() {
        info!("Skipping last-proxy restore while urltest node_select is active");
        return;
    }

    // 等待期间用户可能已切换节点：落地前才读取，避免把旧选择 PUT 回去
    let Some(proxy) = load_last_proxy(&state.runtime_paths.last_proxy).await else {
        return;
    };

    info!(
        "Attempting to restore last proxy: {} -> {}",
        proxy.group, proxy.name
    );

    let url = crate::services::singbox::clash_api_url(&format!(
        "/proxies/{}",
        urlencoding::encode(&proxy.group)
    ));
    let Some(group_payload) = poll_clash_group_for_restore(state, &url, ticket).await else {
        return;
    };

    let group = ClashGroupView(&group_payload);
    if !group.has_members_array() {
        return;
    }
    if !group.contains_member(&proxy.name) {
        warn!(
            "Last proxy '{}' not found in current node list, skipping restore",
            proxy.name
        );
        return;
    }
    if group.is_selected(&proxy.name) {
        info!("Last proxy is already selected: {}", proxy.name);
        return;
    }

    // GET 往返可能耗时数秒：PUT 前最后确认一次这次启动仍是最新的
    if ticket.lifecycle_superseded(state) {
        return;
    }

    match state
        .http_client
        .put(&url)
        .timeout(RESTORE_PUT_TIMEOUT)
        .json(&serde_json::json!({ "name": proxy.name }))
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            info!("Successfully restored last proxy: {}", proxy.name);
        }
        Ok(res) => {
            warn!("Failed to restore last proxy: status {}", res.status());
        }
        Err(e) => {
            error!("Failed to restore last proxy: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::write_last_proxy_file;
    use crate::models::LastProxy;

    #[tokio::test]
    async fn restore_uses_runtime_paths_and_skips_missing_file_without_clash() {
        let state = crate::test_support::app_state(crate::models::Config::default());
        assert_eq!(
            state.runtime_paths.last_proxy,
            state.runtime_paths.runtime_dir.join(".last_proxy")
        );
        assert!(!state.runtime_paths.last_proxy.exists());

        let start = std::time::Instant::now();
        super::restore_last_proxy(&state, 0).await;
        assert!(
            start.elapsed() < std::time::Duration::from_millis(500),
            "missing isolated last-proxy must not probe Clash"
        );
    }

    #[tokio::test]
    async fn restore_aborts_immediately_when_start_is_superseded() {
        let state = crate::test_support::app_state(crate::models::Config::default());
        // 任务监护的 generation=0 已被 generation=1 取代：应立即返回，
        // 不睡 1s、不读 .last_proxy、不碰网络
        while state.lifecycle.snapshot().generation < 1 {
            state
                .lifecycle
                .begin(crate::state::lifecycle::KernelOperation::Start);
        }

        let start = std::time::Instant::now();
        super::restore_last_proxy(&state, 0).await;

        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[tokio::test]
    async fn write_last_proxy_creates_missing_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "harbor-last-proxy-{}-{}",
            std::process::id(),
            "mkdir"
        ));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        let path = dir.join("nested").join(".last_proxy");
        let proxy = LastProxy {
            group: "proxy".to_string(),
            name: "node-a".to_string(),
        };

        write_last_proxy_file(&path, &proxy).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("node-a"));
        assert!(content.contains("proxy"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
    #[tokio::test]
    async fn concurrent_switches_persist_the_final_runtime_selection() {
        use axum::{routing::get, Json, Router};
        let current = std::sync::Arc::new(tokio::sync::Mutex::new("a".to_string()));
        let read = current.clone();
        let write = current.clone();
        let app = Router::new().route("/proxies/proxy", get(move || {
            let current = read.clone();
            async move { Json(serde_json::json!({"type":"Selector","all":["a","b","c"],"now":*current.lock().await})) }
        }).put(move |Json(body): Json<serde_json::Value>| {
            let current = write.clone();
            async move { *current.lock().await = body["name"].as_str().unwrap().to_string(); axum::http::StatusCode::NO_CONTENT }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let state = crate::test_support::app_state(crate::models::Config::default());
        state.lifecycle.finish(
            state.lifecycle.snapshot().generation,
            crate::models::RuntimePhase::Ready,
        );
        let b = LastProxy {
            group: "proxy".to_string(),
            name: "b".to_string(),
        };
        let c = LastProxy {
            group: "proxy".to_string(),
            name: "c".to_string(),
        };
        let (first, second) = tokio::join!(
            super::switch_proxy_at(&state, &b, &base),
            super::switch_proxy_at(&state, &c, &base)
        );
        assert!(first.unwrap().persisted);
        assert!(second.unwrap().persisted);
        let saved = super::load_last_proxy(&state.runtime_paths.last_proxy)
            .await
            .unwrap();
        assert_eq!(saved.name, *current.lock().await);
        // Old restore work is retired even though the kernel generation is unchanged.
        super::restore_last_proxy_if_current(&state, 0, 0).await;
        assert_eq!(saved.name, *current.lock().await);
        server.abort();
    }

    #[tokio::test]
    async fn switch_proxy_skips_put_when_unchanged_and_rejects_regional_preference() {
        use axum::{routing::get, Json, Router};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let puts = std::sync::Arc::new(AtomicUsize::new(0));
        let put_counter = puts.clone();
        let app = Router::new().route(
            "/proxies/proxy",
            get(|| async {
                Json(serde_json::json!({"type":"Selector","all":["a","b"],"now":"a"}))
            })
            .put(move || {
                let put_counter = put_counter.clone();
                async move {
                    put_counter.fetch_add(1, Ordering::Relaxed);
                    axum::http::StatusCode::NO_CONTENT
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let state = crate::test_support::app_state(crate::models::Config::default());
        state.lifecycle.finish(
            state.lifecycle.snapshot().generation,
            crate::models::RuntimePhase::Ready,
        );

        let same = LastProxy {
            group: "proxy".to_string(),
            name: "a".to_string(),
        };
        let result = super::switch_proxy_at(&state, &same, &base).await.unwrap();
        assert!(!result.changed);
        assert!(result.persisted);
        assert_eq!(puts.load(Ordering::Relaxed), 0);

        *state.node_select_preference.write().await =
            crate::models::NodeSelect::Fastest(crate::models::Region::Jp);
        let err = super::switch_proxy_at(&state, &same, &base)
            .await
            .err()
            .expect("regional preference must reject manual switch");
        assert!(err.to_string().contains("手动选择模式"));
        server.abort();
    }
}
