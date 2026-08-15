use futures::{stream, StreamExt};
use std::{
    collections::HashSet,
    path::Path,
    sync::{atomic::Ordering, Arc},
};
use tokio::time::Duration;
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::{Config, RouteMode, SubStatus};
use crate::services::{
    proxy::restore_last_proxy,
    singbox::{
        get_sing_box_home, start_sing_internal, stop_sing_internal, validate_sing_box_config,
    },
    subscription::fetch_sub,
};
use crate::state::AppState;

const CONFIG_CACHE_PATH: &str = "/tmp/miao-sing-box/config.json.cache";
const MAX_CONCURRENT_SUBS: usize = 5;

/// 原子写入文件：先写入临时文件，再重命名为目标文件
async fn write_file_atomic(path: &Path, content: &str) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::context("Failed to create config directory", e))?;
    }

    let temp_path = path.with_extension("tmp");

    // 先写入临时文件
    tokio::fs::write(&temp_path, content)
        .await
        .map_err(|e| AppError::context("Failed to write temp file", e))?;

    // 原子重命名为最终文件
    tokio::fs::rename(&temp_path, path)
        .await
        .map_err(|e| AppError::context("Failed to atomically rename file", e))?;

    Ok(())
}

pub async fn save_config_to(path: &Path, config: &Config) -> AppResult<()> {
    let yaml = serde_yaml::to_string(config)?;
    if let Ok(existing) = tokio::fs::read_to_string(path).await {
        if existing == yaml {
            info!(config_path = ?path, "Config file already up to date, skipping write");
            return Ok(());
        }
    }

    write_file_atomic(path, &yaml).await
}

pub async fn save_config_cache() {
    let config_path = get_sing_box_home().join("config.json");
    if let Err(e) = tokio::fs::copy(&config_path, CONFIG_CACHE_PATH).await {
        error!("Failed to save config cache: {}", e);
    } else {
        info!("Config cache saved to {}", CONFIG_CACHE_PATH);
    }
}

pub async fn restore_config_from_cache() -> AppResult<()> {
    let cache = std::path::Path::new(CONFIG_CACHE_PATH);
    if !cache.exists() {
        return Err(AppError::message("No cached config available"));
    }
    let config_path = get_sing_box_home().join("config.json");
    tokio::fs::copy(CONFIG_CACHE_PATH, &config_path)
        .await
        .map_err(|e| AppError::context("Failed to restore config from cache", e))?;
    info!("Restored config from cache");
    Ok(())
}

pub async fn regenerate_and_restart_runtime(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let has_sub_nodes = gen_config(config, state)
        .await
        .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    validate_sing_box_config()
        .await
        .map_err(|e| AppError::context("Config validation failed, not restarting", e))?;

    stop_sing_internal(state).await;

    start_sing_internal(state)
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box", e))?;
    info!("sing-box restarted successfully");

    Ok(has_sub_nodes)
}

pub async fn regenerate_preserving_service_state(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let route_override = *state.route_mode_override.read().await;
    let runtime_config = config_with_route_override(config, route_override);
    let should_run = state.service_should_run.load(Ordering::Relaxed);

    if config_apply_mode(&runtime_config, should_run) == ConfigApplyMode::Clear {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(false);
    }

    if should_run {
        let has_sub_nodes = regenerate_and_restart_runtime(&runtime_config, state).await?;
        finalize_started_config(&runtime_config, state, has_sub_nodes).await;
    } else {
        let has_sub_nodes = regenerate_without_restart_runtime(&runtime_config, state).await?;
        update_config_warning(&runtime_config, state, has_sub_nodes).await;
    }

    Ok(should_run)
}

pub async fn finalize_started_config(config: &Config, state: &Arc<AppState>, has_sub_nodes: bool) {
    update_config_warning(config, state, has_sub_nodes).await;

    let state_for_proxy = state.clone();
    tokio::spawn(async move {
        restore_last_proxy(&state_for_proxy).await;
    });
}

async fn update_config_warning(config: &Config, state: &Arc<AppState>, has_sub_nodes: bool) {
    save_config_cache().await;

    if has_sub_nodes {
        *state.config_warning.lock().await = None;
    } else if !config.subs.is_empty() {
        *state.config_warning.lock().await = Some("所有订阅获取失败，请检查当前订阅".to_string());
    } else {
        *state.config_warning.lock().await = None;
    }
}

pub async fn regenerate_without_restart_runtime(
    config: &Config,
    state: &Arc<AppState>,
) -> AppResult<bool> {
    let has_sub_nodes = gen_config(config, state)
        .await
        .map_err(|e| AppError::context("Failed to regenerate config", e))?;
    info!("Config regenerated successfully");

    validate_sing_box_config()
        .await
        .map_err(|e| AppError::context("Config validation failed", e))?;

    Ok(has_sub_nodes)
}

fn config_with_route_override(config: &Config, route_mode: Option<RouteMode>) -> Config {
    let mut config = config.clone();
    config.route_mode = route_mode.unwrap_or_default();
    config
}

fn has_configured_sources(config: &Config) -> bool {
    !config.subs.is_empty() || !config.nodes.is_empty()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConfigApplyMode {
    Clear,
    Restart,
    RegenerateOnly,
}

fn config_apply_mode(config: &Config, should_run: bool) -> ConfigApplyMode {
    if !has_configured_sources(config) {
        ConfigApplyMode::Clear
    } else if should_run {
        ConfigApplyMode::Restart
    } else {
        ConfigApplyMode::RegenerateOnly
    }
}

async fn remove_runtime_config_files_at(runtime_config_path: &Path, cache_path: &Path) {
    for path in [runtime_config_path, cache_path] {
        if let Err(err) = tokio::fs::remove_file(path).await {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!(path = ?path, error = %err, "Failed to remove stale runtime config");
            }
        }
    }
}

async fn remove_runtime_config_files() {
    let runtime_config_path = get_sing_box_home().join("config.json");
    remove_runtime_config_files_at(&runtime_config_path, Path::new(CONFIG_CACHE_PATH)).await;
}

async fn clear_runtime_config(state: &Arc<AppState>) {
    remove_runtime_config_files().await;
    state.sub_status.lock().await.clear();
    *state.config_warning.lock().await = None;
}

fn no_usable_nodes_warning(config: &Config) -> String {
    if config.subs.is_empty() {
        "没有可用的手动节点，请检查配置或添加节点".to_string()
    } else {
        "所有订阅获取失败且没有可用手动节点，请检查订阅或添加节点".to_string()
    }
}

async fn persist_config_without_usable_nodes_at(
    state: &Arc<AppState>,
    persisted_config: Config,
    runtime_config_path: &Path,
    cache_path: &Path,
) -> AppResult<()> {
    save_config_to(&state.config_path, &persisted_config).await?;
    stop_sing_internal(state).await;
    remove_runtime_config_files_at(runtime_config_path, cache_path).await;
    *state.config.write().await = persisted_config.clone();
    *state.config_warning.lock().await = Some(no_usable_nodes_warning(&persisted_config));
    Ok(())
}

async fn persist_config_without_usable_nodes(
    state: &Arc<AppState>,
    persisted_config: Config,
) -> AppResult<()> {
    let runtime_config_path = get_sing_box_home().join("config.json");
    persist_config_without_usable_nodes_at(
        state,
        persisted_config,
        &runtime_config_path,
        Path::new(CONFIG_CACHE_PATH),
    )
    .await
}

async fn restore_previous_config(
    old_config: &Config,
    state: &Arc<AppState>,
    should_run: bool,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        return Ok(());
    }

    if should_run {
        restore_previous_running_config(old_config, state).await
    } else {
        restore_previous_stopped_config(old_config, state).await
    }
}

pub async fn apply_config_change(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
) -> AppResult<()> {
    let route_override = *state.route_mode_override.read().await;
    let runtime_old_config = config_with_route_override(old_config, route_override);
    let runtime_new_config = config_with_route_override(new_config, route_override);
    let persisted_new_config = config_with_route_override(new_config, None);
    let should_run = state.service_should_run.load(Ordering::Relaxed);
    let apply_mode = config_apply_mode(&runtime_new_config, should_run);

    if apply_mode == ConfigApplyMode::Clear {
        save_config_to(&state.config_path, &persisted_new_config).await?;
        stop_sing_internal(state).await;
        clear_runtime_config(state).await;
        *state.config.write().await = persisted_new_config;
        return Ok(());
    }

    let apply_result = match apply_mode {
        ConfigApplyMode::Restart => {
            regenerate_and_restart_runtime(&runtime_new_config, state).await
        }
        ConfigApplyMode::RegenerateOnly => {
            regenerate_without_restart_runtime(&runtime_new_config, state).await
        }
        ConfigApplyMode::Clear => unreachable!("clear mode handled above"),
    };

    match apply_result {
        Ok(has_sub_nodes) => {
            match save_config_to(&state.config_path, &persisted_new_config).await {
                Ok(()) => {
                    *state.config.write().await = persisted_new_config;
                    if should_run {
                        finalize_started_config(&runtime_new_config, state, has_sub_nodes).await;
                    } else {
                        update_config_warning(&runtime_new_config, state, has_sub_nodes).await;
                    }
                    Ok(())
                }
                Err(save_err) => {
                    error!(error = %save_err, "Runtime config applied but persistent config write failed, attempting runtime rollback");
                    match restore_previous_config(&runtime_old_config, state, should_run).await {
                        Ok(()) => Err(AppError::context(
                            "Failed to persist config change; restored previous runtime config",
                            save_err,
                        )),
                        Err(rollback_err) => Err(AppError::message(format!(
                            "Failed to persist config change: {}. Runtime rollback failed: {}",
                            save_err, rollback_err
                        ))),
                    }
                }
            }
        }
        Err(apply_err) if apply_err.is_no_usable_nodes() => {
            warn!(error = %apply_err, "Config change left no usable nodes; persisting it and stopping sing-box");
            persist_config_without_usable_nodes(state, persisted_new_config).await
        }
        Err(apply_err) => {
            error!(error = %apply_err, "Failed to apply runtime config change, attempting runtime rollback");
            match restore_previous_config(&runtime_old_config, state, should_run).await {
                Ok(()) => Err(AppError::context(
                    "Failed to apply config change; restored previous runtime config",
                    apply_err,
                )),
                Err(rollback_err) => Err(AppError::message(format!(
                    "Failed to apply config change: {}. Runtime rollback failed: {}",
                    apply_err, rollback_err
                ))),
            }
        }
    }
}

pub async fn apply_runtime_config_change(
    state: &Arc<AppState>,
    old_config: &Config,
    new_config: &Config,
    restart: bool,
) -> AppResult<()> {
    if restart {
        match regenerate_and_restart_runtime(new_config, state).await {
            Ok(has_sub_nodes) => {
                *state.route_mode_override.write().await = Some(new_config.route_mode);
                finalize_started_config(new_config, state, has_sub_nodes).await;
                Ok(())
            }
            Err(apply_err) => {
                error!(error = %apply_err, "Failed to apply runtime-only config change, attempting runtime rollback");
                match restore_previous_running_config(old_config, state).await {
                    Ok(()) => Err(AppError::context(
                        "Failed to apply runtime-only config change; restored previous runtime config",
                        apply_err,
                    )),
                    Err(rollback_err) => Err(AppError::message(format!(
                        "Failed to apply runtime-only config change: {}. Runtime rollback failed: {}",
                        apply_err, rollback_err
                    ))),
                }
            }
        }
    } else {
        match regenerate_without_restart_runtime(new_config, state).await {
            Ok(has_sub_nodes) => {
                *state.route_mode_override.write().await = Some(new_config.route_mode);
                update_config_warning(new_config, state, has_sub_nodes).await;
                Ok(())
            }
            Err(apply_err) => {
                let _ = restore_previous_stopped_config(old_config, state).await;
                Err(AppError::context(
                    "Failed to apply runtime-only config change",
                    apply_err,
                ))
            }
        }
    }
}

async fn sing_box_is_running(state: &Arc<AppState>) -> bool {
    let mut lock = state.sing_process.lock().await;
    match &mut *lock {
        Some(proc) => match proc.child.try_wait() {
            Ok(Some(_)) => {
                *lock = None;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        },
        None => false,
    }
}

async fn restore_previous_running_config(
    old_config: &Config,
    state: &Arc<AppState>,
) -> AppResult<()> {
    if sing_box_is_running(state).await {
        match restore_config_from_cache().await {
            Ok(()) => {}
            Err(cache_err) => {
                warn!(error = %cache_err, "Failed to restore runtime config from cache while previous sing-box process is still running");
                let has_sub_nodes = regenerate_without_restart_runtime(old_config, state).await?;
                update_config_warning(old_config, state, has_sub_nodes).await;
            }
        }
        return Ok(());
    }

    restart_with_previous_config(old_config, state).await
}

async fn restart_with_previous_config(old_config: &Config, state: &Arc<AppState>) -> AppResult<()> {
    stop_sing_internal(state).await;

    if let Err(cache_err) = restore_config_from_cache().await {
        warn!(error = %cache_err, "Failed to restore runtime config from cache for rollback; regenerating previous config");
    } else {
        match start_sing_internal(state).await {
            Ok(()) => {
                finalize_started_config(old_config, state, true).await;
                return Ok(());
            }
            Err(start_err) => {
                warn!(error = %start_err, "Failed to restart sing-box from cached config; regenerating previous config");
            }
        }
    }

    let has_sub_nodes = regenerate_without_restart_runtime(old_config, state).await?;
    start_sing_internal(state)
        .await
        .map_err(|e| AppError::context("Failed to restart sing-box with previous config", e))?;
    finalize_started_config(old_config, state, has_sub_nodes).await;
    Ok(())
}

async fn restore_previous_stopped_config(
    old_config: &Config,
    state: &Arc<AppState>,
) -> AppResult<()> {
    if !has_configured_sources(old_config) {
        clear_runtime_config(state).await;
        return Ok(());
    }

    let has_sub_nodes = regenerate_without_restart_runtime(old_config, state).await?;
    update_config_warning(old_config, state, has_sub_nodes).await;
    Ok(())
}

/// Returns `true` if at least one subscription node was fetched successfully.
pub async fn gen_config(config: &Config, state: &Arc<AppState>) -> AppResult<bool> {
    let (my_outbounds, my_names) = collect_manual_outbounds(config);
    let mut final_outbounds: Vec<serde_json::Value> = vec![];
    let mut final_node_names: Vec<String> = vec![];

    {
        let mut status_map = state.sub_status.lock().await;
        status_map.retain(|url, _| config.subs.contains(url));
    }

    let sub_futures: Vec<_> = config
        .subs
        .iter()
        .map(|sub| {
            let sub = sub.clone();
            let client = state.http_client.clone();
            async move {
                info!(url = %sub, "Fetching subscription");
                let result =
                    tokio::time::timeout(Duration::from_secs(30), fetch_sub(&sub, &client)).await;

                match result {
                    Ok(Ok(fetch_result)) => {
                        let valid_count = fetch_result.node_names.len();
                        let total_count = fetch_result.total_count;
                        let error_count = fetch_result.parse_errors.len();

                        if error_count > 0 {
                            warn!(
                                url = %sub,
                                valid = valid_count,
                                total = total_count,
                                errors = error_count,
                                "Partial fetch: some nodes failed to parse"
                            );
                        } else {
                            info!(
                                url = %sub,
                                nodes = valid_count,
                                "Subscription fetched successfully"
                            );
                        }

                        (sub.clone(), Ok(fetch_result))
                    }
                    Ok(Err(e)) => {
                        error!(url = %sub, error = %e, "Failed to fetch subscription");
                        (sub.clone(), Err(e.to_string()))
                    }
                    Err(_) => {
                        error!(url = %sub, timeout_secs = 30, "Subscription fetch timed out");
                        (sub.clone(), Err("Request timeout".to_string()))
                    }
                }
            }
        })
        .collect();

    // 使用 buffer_unordered 限制并发数，避免同时发起过多请求
    let mut results: Vec<_> = stream::iter(sub_futures)
        .buffer_unordered(MAX_CONCURRENT_SUBS)
        .collect()
        .await;

    // 按原始顺序排序结果
    let subs_order: Vec<String> = config.subs.clone();
    results.sort_by_key(|(url, _)| {
        subs_order
            .iter()
            .position(|s| s == url)
            .unwrap_or(usize::MAX)
    });

    for (url, result) in results {
        let status = match result {
            Ok(fetch_result) => {
                let count = fetch_result.node_names.len();
                final_node_names.extend(fetch_result.node_names);
                final_outbounds.extend(fetch_result.outbounds);

                let error_info = if !fetch_result.parse_errors.is_empty() {
                    Some(format!(
                        "{} nodes skipped due to parse errors",
                        fetch_result.parse_errors.len()
                    ))
                } else if count == 0 && fetch_result.total_count > 0 {
                    Some("All nodes invalid (missing required fields)".into())
                } else if count == 0 {
                    Some("No nodes found".into())
                } else {
                    None
                };

                SubStatus {
                    url: url.clone(),
                    success: count > 0,
                    node_count: count,
                    error: error_info,
                }
            }
            Err(e) => SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                error: Some(e),
            },
        };
        state.sub_status.lock().await.insert(url, status);
    }

    let has_sub_nodes = !final_node_names.is_empty();

    let sing_box_config = build_sing_box_config(
        config,
        my_names,
        my_outbounds,
        final_node_names,
        final_outbounds,
    )?;

    let sing_box_home = get_sing_box_home();
    let config_output_loc = sing_box_home.join("config.json");
    write_file_atomic(
        &config_output_loc,
        &serde_json::to_string(&sing_box_config)?,
    )
    .await?;

    Ok(has_sub_nodes)
}

fn collect_manual_outbounds(config: &Config) -> (Vec<serde_json::Value>, Vec<String>) {
    use crate::services::node_parser::parse_node_json;

    let mut my_outbounds = vec![];
    let mut my_names = vec![];

    for (idx, node_str) in config.nodes.iter().enumerate() {
        // 验证节点并获取解析后的 Value
        match parse_node_json(node_str) {
            Ok((info, outbound)) => {
                my_names.push(info.tag);
                my_outbounds.push(outbound);
            }
            Err(e) => {
                warn!("[collect_manual_outbounds] Skipping node #{}: {}", idx, e);
            }
        }
    }

    (my_outbounds, my_names)
}

fn make_unique_tag(tag: &str, used: &mut HashSet<String>) -> String {
    let base = if tag.trim().is_empty() { "node" } else { tag };
    if used.insert(base.to_string()) {
        return base.to_string();
    }

    for index in 2.. {
        let candidate = format!("{base} ({index})");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded duplicate tag search should always find a value")
}

fn normalize_outbound_tags(
    node_names: Vec<String>,
    outbounds: Vec<serde_json::Value>,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let names_len = node_names.len();
    let mut used = HashSet::new();
    // Built-in outbounds from the template already reserve these tags.
    used.insert("proxy".to_string());
    used.insert("direct".to_string());
    let mut unique_names = Vec::with_capacity(outbounds.len());
    let mut unique_outbounds = Vec::with_capacity(outbounds.len());

    for (idx, mut outbound) in outbounds.into_iter().enumerate() {
        let original_name = node_names
            .get(idx)
            .cloned()
            .or_else(|| {
                outbound
                    .get("tag")
                    .and_then(|tag| tag.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("node-{}", idx + 1));
        let unique_name = make_unique_tag(&original_name, &mut used);

        if unique_name != original_name {
            warn!(
                from = %original_name,
                to = %unique_name,
                "Renamed duplicate outbound tag to avoid sing-box conflict"
            );
        }

        if let Some(obj) = outbound.as_object_mut() {
            obj.insert(
                "tag".to_string(),
                serde_json::Value::String(unique_name.clone()),
            );
        } else {
            warn!(tag = %unique_name, "Outbound is not a JSON object; cannot set tag");
        }

        unique_names.push(unique_name);
        unique_outbounds.push(outbound);
    }

    if names_len != unique_outbounds.len() {
        warn!(
            names = names_len,
            outbounds = unique_outbounds.len(),
            "Outbound name count did not match outbound config count"
        );
    }

    (unique_names, unique_outbounds)
}

fn build_sing_box_config(
    config: &Config,
    my_names: Vec<String>,
    my_outbounds: Vec<serde_json::Value>,
    final_node_names: Vec<String>,
    final_outbounds: Vec<serde_json::Value>,
) -> AppResult<serde_json::Value> {
    let total_nodes = my_outbounds.len() + final_outbounds.len();
    if total_nodes == 0 {
        return Err(AppError::NoUsableNodes);
    }

    let (node_names, outbounds) = normalize_outbound_tags(
        my_names.into_iter().chain(final_node_names).collect(),
        my_outbounds.into_iter().chain(final_outbounds).collect(),
    );

    let mut sing_box_config = get_config_template();
    if let Some(selector_outbounds) = sing_box_config["outbounds"][0].get_mut("outbounds") {
        if let Some(arr) = selector_outbounds.as_array_mut() {
            arr.extend(node_names.into_iter().map(serde_json::Value::String));
        }
    }
    if let Some(arr) = sing_box_config["outbounds"].as_array_mut() {
        arr.extend(outbounds);
    }

    apply_route_mode(
        &mut sing_box_config,
        config.route_mode,
        &config.custom_rules,
    );

    Ok(sing_box_config)
}

fn parse_custom_rules(custom_rules: &[String]) -> Vec<serde_json::Value> {
    let mut parsed = Vec::new();
    for rule_str in custom_rules {
        if let Ok(rule_json) = serde_json::from_str::<serde_json::Value>(rule_str) {
            parsed.push(rule_json);
        } else {
            warn!("Failed to parse custom rule: {}", rule_str);
        }
    }
    parsed
}

fn apply_route_mode(
    sing_box_config: &mut serde_json::Value,
    route_mode: RouteMode,
    custom_rules: &[String],
) {
    if let Some(rules) = sing_box_config["route"]["rules"].as_array_mut() {
        match route_mode {
            RouteMode::Rule => {
                let custom_rules = parse_custom_rules(custom_rules);
                // Preserve the mandatory pre-routing actions, then let user rules take
                // precedence over the built-in direct/proxy split rules.
                let insertion_index = rules.len().min(2);
                rules.splice(insertion_index..insertion_index, custom_rules);
            }
            RouteMode::Global => {
                rules.truncate(2);
            }
        }
    }

    if route_mode == RouteMode::Global {
        if let Some(dns_rules) = sing_box_config["dns"]["rules"].as_array_mut() {
            dns_rules.clear();
        }
    }
}

fn get_config_template() -> serde_json::Value {
    serde_json::json!({
        "log": {"disabled": false, "timestamp": true, "level": "info"},
        "experimental": {"clash_api": {"external_controller": "127.0.0.1:6262"}},
        "dns": {
            "final": "cfdns",
            "strategy": "ipv4_only",
            "disable_cache": false,
            "servers": [
                {"type": "https", "tag": "cfdns", "server": "1.1.1.1", "detour": "proxy"},
                {"tag": "local", "type": "udp", "server": "223.5.5.5"}
            ],
            "rules": [
                {"domain_suffix": ["hdslb.com"], "action": "route", "server": "local"},
                {"rule_set": ["chinasite"], "action": "route", "server": "local"}
            ]
        },
        "inbounds": [
            {"type": "tun", "tag": "tun-in", "interface_name": "sing-tun", "address": ["172.18.0.1/30"], "mtu": 9000, "auto_route": true, "strict_route": true, "auto_redirect": true}
        ],
        "outbounds": [
            {"type": "selector", "tag": "proxy", "outbounds": []},
            {"type": "direct", "tag": "direct"}
        ],
        "route": {
            "final": "proxy",
            "auto_detect_interface": true,
            "default_domain_resolver": "local",
            "rules": [
                {"action": "sniff"},
                {"protocol": "dns", "action": "hijack-dns"},
                {"ip_is_private": true, "action": "route", "outbound": "direct"},
                {"domain_suffix": ["hdslb.com"], "action": "route", "outbound": "direct"},
                {"rule_set": ["chinasite"], "action": "route", "outbound": "direct"},
                {"rule_set": ["chinaip"], "action": "route", "outbound": "direct"}
            ],
            "rule_set": [
                {"type": "local", "tag": "chinasite", "format": "binary", "path": "./chinasite.srs"},
                {"type": "local", "tag": "chinaip", "format": "binary", "path": "./chinaip.srs"}
            ]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_sing_box_config, collect_manual_outbounds, config_apply_mode,
        config_with_route_override, no_usable_nodes_warning,
        persist_config_without_usable_nodes_at, save_config_to, ConfigApplyMode,
    };
    use crate::{
        models::{Config, RouteMode, SubStatus},
        test_support::app_state,
    };
    use serde_json::json;

    #[test]
    fn collect_manual_outbounds_ignores_invalid_json_nodes() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                r#"{"type":"hysteria2","tag":"manual-a","server":"a.example.com","server_port":443,"password":"p","up_mbps":40,"down_mbps":350,"tls":{"enabled":true,"insecure":true}}"#.to_string(),
                "{invalid-json".to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        assert_eq!(outbounds.len(), 1);
        assert_eq!(names, vec!["manual-a"]);
        assert_eq!(outbounds[0]["tag"], "manual-a");
    }

    #[test]
    fn collect_manual_outbounds_preserves_hysteria2_without_default_bandwidth() {
        // 测试：Hysteria2 节点不强制包含带宽默认值
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                // 不包含 up_mbps/down_mbps 的节点
                r#"{"type":"hysteria2","tag":"no-bandwidth","server":"example.com","server_port":443,"password":"secret","tls":{"enabled":true}}"#.to_string(),
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        assert_eq!(outbounds.len(), 1);
        assert_eq!(names, vec!["no-bandwidth"]);
        // 验证不包含硬编码的带宽字段
        assert!(outbounds[0].get("up_mbps").is_none() || outbounds[0]["up_mbps"].is_null());
        assert!(outbounds[0].get("down_mbps").is_none() || outbounds[0]["down_mbps"].is_null());
    }

    #[test]
    fn build_sing_box_config_merges_nodes_and_valid_custom_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![
                r#"{"domain_suffix":["example.com"],"action":"route","outbound":"proxy"}"#
                    .to_string(),
                "not-json".to_string(),
            ],
            route_mode: Default::default(),
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];
        let final_outbounds = vec![json!({
            "type": "shadowsocks",
            "tag": "sub-a",
            "server": "sub.example.com",
            "server_port": 8388,
            "method": "2022-blake3-aes-128-gcm",
            "password": "sub-secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec!["sub-a".to_string()],
            final_outbounds,
        )
        .unwrap();

        let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
        assert_eq!(selector.len(), 2);
        assert_eq!(selector[0], "manual-a");
        assert_eq!(selector[1], "sub-a");

        let all_outbounds = built["outbounds"].as_array().unwrap();
        assert_eq!(all_outbounds.len(), 4);
        assert_eq!(all_outbounds[2]["tag"], "manual-a");
        assert_eq!(all_outbounds[3]["tag"], "sub-a");

        let rules = built["route"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 7);
        assert_eq!(rules[0]["action"], "sniff");
        assert_eq!(rules[1]["action"], "hijack-dns");
        assert_eq!(rules[2]["domain_suffix"][0], "example.com");
        assert_eq!(rules[3]["ip_is_private"], true);
    }

    #[test]
    fn build_sing_box_config_global_mode_removes_split_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![
                r#"{"domain_suffix":["example.com"],"action":"route","outbound":"direct"}"#
                    .to_string(),
            ],
            route_mode: RouteMode::Global,
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let rules = built["route"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["action"], "sniff");
        assert_eq!(rules[1]["action"], "hijack-dns");
        assert!(rules.iter().all(|rule| rule.get("outbound").is_none()));

        let dns_rules = built["dns"]["rules"].as_array().unwrap();
        assert!(dns_rules.is_empty());
        assert_eq!(built["route"]["final"], "proxy");
    }

    #[test]
    fn config_change_clears_runtime_when_last_source_is_removed() {
        let config = Config::default();

        assert_eq!(config_apply_mode(&config, true), ConfigApplyMode::Clear);
        assert_eq!(config_apply_mode(&config, false), ConfigApplyMode::Clear);
    }

    #[test]
    fn unusable_node_warning_distinguishes_manual_and_subscription_configs() {
        let manual = Config {
            nodes: vec!["invalid-node".to_string()],
            ..Config::default()
        };
        let subscription = Config {
            subs: vec!["https://example.com/sub".to_string()],
            ..Config::default()
        };

        assert!(no_usable_nodes_warning(&manual).contains("手动节点"));
        assert!(no_usable_nodes_warning(&subscription).contains("订阅"));
    }

    #[tokio::test]
    async fn unusable_config_is_persisted_and_stale_runtime_files_are_removed() {
        let state = app_state(Config::default());
        let temp_dir =
            std::env::temp_dir().join(format!("miao-unusable-config-{}", std::process::id()));
        let runtime_path = temp_dir.join("config.json");
        let cache_path = temp_dir.join("config.json.cache");
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        tokio::fs::write(&runtime_path, "stale").await.unwrap();
        tokio::fs::write(&cache_path, "stale").await.unwrap();

        let subscription_url = "https://example.com/broken".to_string();
        state.sub_status.lock().await.insert(
            subscription_url.clone(),
            SubStatus {
                url: subscription_url.clone(),
                success: false,
                node_count: 0,
                error: Some("fetch failed".to_string()),
            },
        );
        let config = Config {
            subs: vec![subscription_url.clone()],
            ..Config::default()
        };

        persist_config_without_usable_nodes_at(&state, config, &runtime_path, &cache_path)
            .await
            .unwrap();

        assert!(!runtime_path.exists());
        assert!(!cache_path.exists());
        assert_eq!(
            state.config.read().await.subs,
            vec![subscription_url.clone()]
        );
        assert!(state.config_warning.lock().await.is_some());
        assert!(state
            .sub_status
            .lock()
            .await
            .contains_key(&subscription_url));
        let persisted = tokio::fs::read_to_string(&state.config_path).await.unwrap();
        assert!(persisted.contains(&subscription_url));

        let _ = tokio::fs::remove_file(&state.config_path).await;
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }

    #[test]
    fn config_change_preserves_explicitly_stopped_service() {
        let config = Config {
            nodes: vec![r#"{"type":"hysteria2"}"#.to_string()],
            ..Config::default()
        };

        assert_eq!(
            config_apply_mode(&config, false),
            ConfigApplyMode::RegenerateOnly
        );
    }

    #[test]
    fn config_change_restarts_service_when_it_is_desired() {
        let config = Config {
            subs: vec!["https://example.com/sub".to_string()],
            ..Config::default()
        };

        assert_eq!(config_apply_mode(&config, true), ConfigApplyMode::Restart);
    }

    #[test]
    fn config_with_route_override_defaults_to_rule_mode() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: RouteMode::Global,
        };

        let runtime_config = config_with_route_override(&config, None);

        assert_eq!(runtime_config.route_mode, RouteMode::Rule);
    }

    #[test]
    fn build_sing_box_config_renames_duplicate_outbound_tags() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "dup",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "manual-secret"
        })];
        let final_outbounds = vec![
            json!({
                "type": "hysteria2",
                "tag": "dup",
                "server": "sub1.example.com",
                "server_port": 443,
                "password": "sub-secret-1"
            }),
            json!({
                "type": "shadowsocks",
                "tag": "dup",
                "server": "sub2.example.com",
                "server_port": 8388,
                "method": "2022-blake3-aes-128-gcm",
                "password": "sub-secret-2"
            }),
        ];

        let built = build_sing_box_config(
            &config,
            vec!["dup".to_string()],
            my_outbounds,
            vec!["dup".to_string(), "dup".to_string()],
            final_outbounds,
        )
        .unwrap();

        let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
        let selector_tags: Vec<_> = selector
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(selector_tags, vec!["dup", "dup (2)", "dup (3)"]);

        let all_outbounds = built["outbounds"].as_array().unwrap();
        assert_eq!(all_outbounds[2]["tag"], "dup");
        assert_eq!(all_outbounds[3]["tag"], "dup (2)");
        assert_eq!(all_outbounds[4]["tag"], "dup (3)");
    }

    #[test]
    fn build_sing_box_config_renames_tags_reserved_by_template() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let my_outbounds = vec![
            json!({
                "type": "hysteria2",
                "tag": "proxy",
                "server": "proxy.example.com",
                "server_port": 443,
                "password": "proxy-secret"
            }),
            json!({
                "type": "hysteria2",
                "tag": "direct",
                "server": "direct.example.com",
                "server_port": 443,
                "password": "direct-secret"
            }),
        ];

        let built = build_sing_box_config(
            &config,
            vec!["proxy".to_string(), "direct".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
        let selector_tags: Vec<_> = selector
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(selector_tags, vec!["proxy (2)", "direct (2)"]);

        let all_outbounds = built["outbounds"].as_array().unwrap();
        assert_eq!(all_outbounds[0]["tag"], "proxy");
        assert_eq!(all_outbounds[1]["tag"], "direct");
        assert_eq!(all_outbounds[2]["tag"], "proxy (2)");
        assert_eq!(all_outbounds[3]["tag"], "direct (2)");
    }

    #[test]
    fn build_sing_box_config_errors_when_no_nodes_available() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let err = build_sing_box_config(&config, vec![], vec![], vec![], vec![]).unwrap_err();

        assert!(err.is_no_usable_nodes());
        assert!(err.to_string().contains(
            "No usable nodes available: subscriptions failed or manual nodes were invalid"
        ));
    }

    #[test]
    fn collect_manual_outbounds_handles_empty_nodes() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        assert!(outbounds.is_empty());
        assert!(names.is_empty());
    }

    #[test]
    fn collect_manual_outbounds_handles_all_invalid_nodes() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![
                "not-json".to_string(),
                r#"{}"#.to_string(),                   // Valid JSON but no tag
                r#"{"type":"hysteria2"}"#.to_string(), // Valid JSON but no tag
            ],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let (outbounds, names) = collect_manual_outbounds(&config);

        // All nodes fail validation (missing required fields)
        assert!(outbounds.is_empty());
        assert!(names.is_empty());
    }

    #[test]
    fn build_sing_box_config_preserves_node_order() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let my_outbounds = vec![
            json!({"type": "hysteria2", "tag": "node-1", "server": "s1.example.com", "server_port": 443, "password": "p1"}),
            json!({"type": "hysteria2", "tag": "node-2", "server": "s2.example.com", "server_port": 443, "password": "p2"}),
            json!({"type": "hysteria2", "tag": "node-3", "server": "s3.example.com", "server_port": 443, "password": "p3"}),
        ];

        let built = build_sing_box_config(
            &config,
            vec![
                "node-1".to_string(),
                "node-2".to_string(),
                "node-3".to_string(),
            ],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let selector = built["outbounds"][0]["outbounds"].as_array().unwrap();
        assert_eq!(selector.len(), 3);
        assert_eq!(selector[0], "node-1");
        assert_eq!(selector[1], "node-2");
        assert_eq!(selector[2], "node-3");
    }

    #[test]
    fn build_sing_box_config_handles_no_custom_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let rules = built["route"]["rules"].as_array().unwrap();
        // Should have the default direct-split rules.
        assert_eq!(rules.len(), 6);
    }

    #[test]
    fn build_sing_box_config_splits_direct_route_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let rules = built["route"]["rules"].as_array().unwrap();

        assert_eq!(rules[2]["ip_is_private"], true);
        assert_eq!(rules[2]["outbound"], "direct");
        assert!(rules[2].get("rule_set").is_none());

        assert_eq!(rules[3]["domain_suffix"], json!(["hdslb.com"]));
        assert_eq!(rules[3]["outbound"], "direct");
        assert!(rules[3].get("rule_set").is_none());
        assert!(rules[3].get("ip_is_private").is_none());

        assert_eq!(rules[4]["rule_set"], json!(["chinasite"]));
        assert_eq!(rules[4]["outbound"], "direct");
        assert!(rules[4].get("ip_is_private").is_none());

        assert_eq!(rules[5]["rule_set"], json!(["chinaip"]));
        assert_eq!(rules[5]["outbound"], "direct");
        assert!(rules[5].get("ip_is_private").is_none());

        let dns_rules = built["dns"]["rules"].as_array().unwrap();
        assert_eq!(dns_rules.len(), 2);
        assert_eq!(dns_rules[0]["domain_suffix"], json!(["hdslb.com"]));
        assert_eq!(dns_rules[0]["server"], "local");
        assert_eq!(dns_rules[1]["rule_set"], json!(["chinasite"]));
        assert_eq!(dns_rules[1]["server"], "local");

        assert_eq!(built["dns"]["disable_cache"], false);
        assert!(built["dns"].get("cache_capacity").is_none());
        assert!(built["dns"].get("optimistic").is_none());

        let dns_servers = built["dns"]["servers"].as_array().unwrap();
        let cfdns = dns_servers
            .iter()
            .find(|server| server["tag"] == "cfdns")
            .unwrap();
        assert_eq!(cfdns["type"], "https");
        assert_eq!(cfdns["server"], "1.1.1.1");
        assert_eq!(cfdns["detour"], "proxy");

        assert!(dns_servers
            .iter()
            .all(|server| server["type"] != "fakeip" && server["tag"] != "fakeip"));

        assert!(built["experimental"].get("cache_file").is_none());
    }

    #[test]
    fn build_sing_box_config_binds_clash_api_to_localhost() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            vec![json!({
                "type": "hysteria2",
                "tag": "manual-a",
                "server": "manual.example.com",
                "server_port": 443,
                "password": "secret"
            })],
            vec![],
            vec![],
        )
        .unwrap();

        assert_eq!(
            built["experimental"]["clash_api"]["external_controller"],
            "127.0.0.1:6262"
        );
    }

    #[test]
    fn build_sing_box_config_ignores_all_invalid_custom_rules() {
        let config = Config {
            port: None,
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![
                "not-json".to_string(),
                "{invalid".to_string(),
                "".to_string(),
            ],
            route_mode: Default::default(),
        };

        let my_outbounds = vec![json!({
            "type": "hysteria2",
            "tag": "manual-a",
            "server": "manual.example.com",
            "server_port": 443,
            "password": "secret"
        })];

        let built = build_sing_box_config(
            &config,
            vec!["manual-a".to_string()],
            my_outbounds,
            vec![],
            vec![],
        )
        .unwrap();

        let rules = built["route"]["rules"].as_array().unwrap();
        // Should have only the default direct-split rules.
        assert_eq!(rules.len(), 6);
    }

    #[tokio::test]
    async fn save_config_performs_atomic_write() {
        let temp_dir = std::env::temp_dir().join(format!(
            "miao-test-save-{}-{}",
            std::process::id(),
            "atomic"
        ));
        let config_path = temp_dir.join("nested").join("config.yaml");

        let config = Config {
            port: Some(8080),
            subs: vec!["https://example.com/sub".to_string()],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        save_config_to(&config_path, &config).await.unwrap();

        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let parsed: Config = serde_yaml::from_str(&content).unwrap();
        assert_eq!(parsed.port, Some(8080));
        assert_eq!(parsed.subs.len(), 1);

        // 清理
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn save_config_overwrites_existing_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "miao-test-save-{}-{}",
            std::process::id(),
            "overwrite"
        ));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        let config_path = temp_dir.join("config.yaml");

        // 先创建旧配置
        tokio::fs::write(
            &config_path,
            "port: 9999\nsubs: []\nnodes: []\ncustom_rules: []",
        )
        .await
        .unwrap();

        // 使用原子写入保存新配置
        let config = Config {
            port: Some(7777),
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };
        save_config_to(&config_path, &config).await.unwrap();

        let content = tokio::fs::read_to_string(&config_path).await.unwrap();
        let parsed: Config = serde_yaml::from_str(&content).unwrap();
        assert_eq!(parsed.port, Some(7777));

        // 清理
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }

    #[tokio::test]
    async fn save_config_skips_identical_content() {
        let temp_dir =
            std::env::temp_dir().join(format!("miao-test-save-{}-{}", std::process::id(), "skip"));
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();

        let config_path = temp_dir.join("config.yaml");
        let config = Config {
            port: Some(6161),
            subs: vec![],
            nodes: vec![],
            custom_rules: vec![],
            route_mode: Default::default(),
        };

        save_config_to(&config_path, &config).await.unwrap();
        let before = tokio::fs::metadata(&config_path)
            .await
            .unwrap()
            .modified()
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        save_config_to(&config_path, &config).await.unwrap();

        let after = tokio::fs::metadata(&config_path)
            .await
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
