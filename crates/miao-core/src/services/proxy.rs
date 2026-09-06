use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{LastProxy, SwitchProxyResult};
use crate::state::AppState;

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
    state
        .proxy_selection_generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    if proxy.group != "proxy" || !state.node_select_preference.read().await.is_manual() {
        return Err(AppError::message(
            "当前是地区最快模式或无效分组，请先切换为手动选择模式",
        ));
    }
    if !state.lifecycle.snapshot().ready {
        return Err(AppError::message("服务未运行或代理数据面尚未就绪"));
    }
    let url = format!("{base}/proxies/{}", urlencoding::encode(&proxy.group));
    let info: serde_json::Value = state
        .http_client
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if info["type"] != "Selector"
        || !info["all"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|node| node.as_str() == Some(&proxy.name)))
    {
        return Err(AppError::message("节点不在当前手动选择列表中"));
    }
    let changed = info["now"].as_str() != Some(&proxy.name);
    if changed {
        state
            .http_client
            .put(&url)
            .timeout(Duration::from_secs(2))
            .json(&serde_json::json!({"name":proxy.name}))
            .send()
            .await?
            .error_for_status()?;
    }
    // Also supersede old restore work when persistence fails: an accepted
    // runtime selection must not be replaced by a previously stored choice.
    state
        .proxy_selection_generation
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let persisted = write_last_proxy_file(&state.runtime_paths.last_proxy, proxy)
        .await
        .is_ok();
    Ok(SwitchProxyResult { changed, persisted })
}

async fn load_last_proxy(path: &Path) -> Option<LastProxy> {
    if let Ok(content) = tokio::fs::read_to_string(path).await {
        serde_json::from_str(&content).ok()
    } else {
        None
    }
}

/// 内核启动或配置重载后调用：捕获当前 lifecycle generation 并派生恢复任务。
/// 连续两次运行配置激活会各派生一个任务；旧任务在落地前发现自己监护的代次
/// 已被取代即放弃，避免陈旧的 PUT 覆盖更新那次启动的选择。
pub fn spawn_restore_last_proxy(state: &Arc<AppState>) {
    let generation = state.lifecycle.snapshot().generation;
    let selection = state
        .proxy_selection_generation
        .load(std::sync::atomic::Ordering::Relaxed);
    let state = state.clone();
    tokio::spawn(async move {
        restore_last_proxy_if_current(&state, generation, selection).await;
    });
}

fn is_superseded(state: &AppState, generation: u64) -> bool {
    state.lifecycle.snapshot().generation != generation
}

#[cfg(test)]
async fn restore_last_proxy(state: &Arc<AppState>, generation: u64) {
    let selection = state
        .proxy_selection_generation
        .load(std::sync::atomic::Ordering::Relaxed);
    restore_last_proxy_if_current(state, generation, selection).await;
}

async fn restore_last_proxy_if_current(state: &Arc<AppState>, generation: u64, selection: u64) {
    let _guard = state.config_update.lock().await;
    if state
        .proxy_selection_generation
        .load(std::sync::atomic::Ordering::Relaxed)
        != selection
    {
        return;
    }
    if is_superseded(state, generation) {
        return;
    }

    if !state.config.read().await.node_select.is_manual() {
        info!("Skipping last-proxy restore while urltest node_select is active");
        return;
    }

    // 等待期间用户可能已切换节点：落地前才读取，避免把旧选择 PUT 回去
    let proxy = match load_last_proxy(&state.runtime_paths.last_proxy).await {
        Some(p) => p,
        None => return,
    };

    info!(
        "Attempting to restore last proxy: {} -> {}",
        proxy.group, proxy.name
    );

    let url = crate::services::singbox::clash_api_url(&format!(
        "/proxies/{}",
        urlencoding::encode(&proxy.group)
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    let group_info = loop {
        if is_superseded(state, generation) {
            info!("Skipping last-proxy restore: superseded by a newer sing-box start");
            return;
        }
        match state
            .http_client
            .get(&url)
            .timeout(Duration::from_millis(300))
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => match res.json::<serde_json::Value>().await {
                Ok(value) => break value,
                Err(_) => return,
            },
            _ if Instant::now() < deadline => sleep(Duration::from_millis(50)).await,
            _ => return,
        }
    };

    let all_nodes = group_info.get("all").and_then(|v| v.as_array());
    if let Some(nodes) = all_nodes {
        let node_exists = nodes.iter().any(|n| n.as_str() == Some(&proxy.name));
        if !node_exists {
            warn!(
                "Last proxy '{}' not found in current node list, skipping restore",
                proxy.name
            );
            return;
        }
    } else {
        return;
    }

    if group_info.get("now").and_then(|value| value.as_str()) == Some(&proxy.name) {
        info!("Last proxy is already selected: {}", proxy.name);
        return;
    }

    // GET 往返可能耗时数秒：PUT 前最后确认一次这次启动仍是最新的
    if is_superseded(state, generation) {
        return;
    }

    match state
        .http_client
        .put(&url)
        .timeout(Duration::from_secs(1))
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
            "miao-last-proxy-{}-{}",
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
}
