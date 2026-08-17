use futures::{stream, StreamExt};
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{error, info, warn};

use crate::error::AppResult;
use crate::models::{Config, NodeSelect, SubStatus};
use crate::services::singbox::get_sing_box_home;
use crate::services::subscription::fetch_sub;
use crate::state::AppState;

use super::builder::build_sing_box_config;
use super::persist::write_file_atomic;

pub struct GenConfigOutcome {
    pub has_sub_nodes: bool,
    pub node_select: NodeSelect,
}

const MAX_CONCURRENT_SUBS: usize = 5;

/// 订阅全失败时的退避重试预算。
/// 开机竞速（miao 先于默认路由/DHCP 就绪启动）时订阅请求会全部秒败；
/// 预算内的退避重试可以跨过这个窗口。手动刷新传 None：用户在场，失败即报。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubFetchRetry {
    /// 不重试
    #[default]
    None,
    /// 启动路径预算：见 STARTUP_RETRY_SCHEDULE
    Startup,
}

/// Startup 预算的退避序列：总等待 50s，与 install.sh ExecStartPre 的 60s 路由等待对齐
const STARTUP_RETRY_SCHEDULE: &[Duration] = &[
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// 测试里缩短以保持用例快速（与 singbox.rs 的 KERNEL_WATCH_INTERVAL 同款手法）
#[cfg(test)]
const STARTUP_RETRY_SCHEDULE_TEST: &[Duration] =
    &[Duration::from_millis(50), Duration::from_millis(100)];

#[cfg(not(test))]
fn startup_retry_schedule() -> &'static [Duration] {
    STARTUP_RETRY_SCHEDULE
}

#[cfg(test)]
fn startup_retry_schedule() -> &'static [Duration] {
    STARTUP_RETRY_SCHEDULE_TEST
}

impl SubFetchRetry {
    fn schedule(self) -> &'static [Duration] {
        match self {
            Self::None => &[],
            Self::Startup => startup_retry_schedule(),
        }
    }
}

/// 是否值得为这次结果退避重试：配置了订阅却一个订阅节点都没拿到才算
/// （全部秒败是网络未就绪的典型瞬态）；部分成功/其他错误更像订阅本身坏了，直接返回
fn should_retry_sub_fetch(result: &AppResult<GenConfigOutcome>, subs_configured: bool) -> bool {
    if !subs_configured {
        return false;
    }
    match result {
        Ok(outcome) => !outcome.has_sub_nodes,
        Err(err) => err.is_no_usable_nodes(),
    }
}

/// 拉取订阅并写出 sing-box 配置；订阅全失败时按 retry 预算退避重试。
/// 返回是否拿到订阅节点，以及实际生效的 node_select。
pub async fn gen_config(
    config: &Config,
    state: &Arc<AppState>,
    retry: SubFetchRetry,
) -> AppResult<GenConfigOutcome> {
    let schedule = retry.schedule();
    let mut attempt = 0usize;
    loop {
        let result = gen_config_once(config, state).await;
        if !should_retry_sub_fetch(&result, !config.subs.is_empty()) {
            return result;
        }
        let Some(delay) = schedule.get(attempt) else {
            return result;
        };
        attempt += 1;
        info!(
            delay_ms = delay.as_millis(),
            attempt, "All subscriptions failed; retrying after backoff"
        );
        tokio::time::sleep(*delay).await;
    }
}

async fn gen_config_once(config: &Config, state: &Arc<AppState>) -> AppResult<GenConfigOutcome> {
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

    let (sing_box_config, skipped_rules, node_select) = build_sing_box_config(
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

    *state.skipped_rules.lock().await = skipped_rules;

    Ok(GenConfigOutcome {
        has_sub_nodes,
        node_select,
    })
}

pub(super) fn collect_manual_outbounds(config: &Config) -> (Vec<serde_json::Value>, Vec<String>) {
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

/// 从运行时 sing-box 配置中提取用户节点 tag(排除内置 outbound)
pub(super) fn runtime_config_node_tags(config_json: &serde_json::Value) -> Vec<String> {
    config_json["outbounds"]
        .as_array()
        .map(|outbounds| {
            outbounds
                .iter()
                .filter_map(|outbound| outbound["tag"].as_str())
                .filter(|tag| *tag != "proxy" && *tag != "direct")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 自定义规则可选的节点目标:手动节点(配置直读)+ 订阅节点(最近一次生成的运行时配置)。
/// 仅供规则校验给出友好报错,最终正确性由 sing-box check 保证。
pub async fn known_rule_targets(config: &Config) -> Vec<String> {
    use crate::services::node_parser::parse_node_json;

    let mut tags: Vec<String> = Vec::new();
    for node_str in &config.nodes {
        if let Ok((info, _)) = parse_node_json(node_str) {
            if !tags.contains(&info.tag) {
                tags.push(info.tag);
            }
        }
    }

    let runtime_config_path = get_sing_box_home().join("config.json");
    if let Ok(content) = tokio::fs::read_to_string(&runtime_config_path).await {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            for tag in runtime_config_node_tags(&json) {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            }
        }
    }

    tags
}

#[cfg(test)]
mod tests {
    use super::{should_retry_sub_fetch, GenConfigOutcome, SubFetchRetry, STARTUP_RETRY_SCHEDULE};
    use crate::error::AppError;
    use crate::models::{Config, NodeSelect};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::time::Duration;

    fn outcome(has_sub_nodes: bool) -> GenConfigOutcome {
        GenConfigOutcome {
            has_sub_nodes,
            node_select: NodeSelect::Manual,
        }
    }

    #[test]
    fn startup_retry_schedule_is_bounded_under_a_minute() {
        assert_eq!(
            STARTUP_RETRY_SCHEDULE,
            &[
                Duration::from_secs(5),
                Duration::from_secs(15),
                Duration::from_secs(30)
            ]
        );
    }

    #[test]
    fn retry_only_applies_to_total_subscription_failure() {
        assert!(should_retry_sub_fetch(&Ok(outcome(false)), true));
        assert!(!should_retry_sub_fetch(&Ok(outcome(true)), true));
        assert!(should_retry_sub_fetch(&Err(AppError::NoUsableNodes), true));
        assert!(!should_retry_sub_fetch(
            &Err(AppError::message("boom")),
            true
        ));
        // 未配置订阅时「全失败」不是瞬态，重试无意义
        assert!(!should_retry_sub_fetch(&Ok(outcome(false)), false));
    }

    /// 本地「计数拒答」订阅服务器：每接受一个 TCP 连接就计数并立即挂断，
    /// 让 fetch 以连接错误快速失败。用拉取次数而非耗时断言重试行为，
    /// 跨平台无时间敏感（Windows CI 上 loopback 拒连耗时不稳定）
    async fn counting_sub_server(attempts: Arc<AtomicUsize>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind counting server");
        let port = listener.local_addr().expect("local addr").port();
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                attempts.fetch_add(1, Ordering::Relaxed);
                drop(socket);
            }
        });
        format!("http://127.0.0.1:{port}/sub")
    }

    #[tokio::test]
    async fn startup_retries_total_failure_until_budget_exhausted() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let url = counting_sub_server(attempts.clone()).await;
        let config = Config {
            subs: vec![url],
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());

        // 无手动节点时 gen_config_once 在写盘前返回 NoUsableNodes，不触碰 /tmp/miao-sing-box
        let err = match tokio::time::timeout(
            Duration::from_secs(5),
            super::gen_config(&config, &state, SubFetchRetry::Startup),
        )
        .await
        {
            Ok(Ok(_)) => panic!("failing subscription must fail"),
            Ok(Err(err)) => err,
            Err(_) => panic!("gen_config exceeded 5s with the shortened test schedule"),
        };

        assert!(err.is_no_usable_nodes());
        // 初次 + 测试调度（50ms/100ms）的两段退避 = 恰好 3 次拉取
        assert_eq!(attempts.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn no_retry_returns_after_a_single_attempt() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let url = counting_sub_server(attempts.clone()).await;
        let config = Config {
            subs: vec![url],
            ..Config::default()
        };
        let state = crate::test_support::app_state(config.clone());

        let err = match tokio::time::timeout(
            Duration::from_secs(5),
            super::gen_config(&config, &state, SubFetchRetry::None),
        )
        .await
        {
            Ok(Ok(_)) => panic!("failing subscription must fail"),
            Ok(Err(err)) => err,
            Err(_) => panic!("gen_config exceeded 5s without any retry budget"),
        };

        assert!(err.is_no_usable_nodes());
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }
}
