use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::{
    command_error, success, success_no_data, CommandErrorKind, CommandReply, CommandResult,
};
use crate::models::{
    DisabledNode, SetNodeDisabledRequest, SubBatchRequest, SubBatchResult, SubNodeInfo,
    SubNodesInfo, SubRequest, SubStatus, SubscriptionState, VergeImportItem, VergeImportResult,
};
use crate::services::config::{
    apply_disabled_nodes, collect_manual_outbounds, edit_subscriptions, read_sub_nodes_snapshot,
    refresh_subscriptions_foreground, subscription_source_id, ConfigMutationError,
};
use crate::services::subscription::is_informational_subscription_node;
use crate::services::verge;
use crate::state::AppState;
use crate::validation::Validator;

pub async fn get_subs(state: Arc<AppState>) -> CommandReply<Vec<SubStatus>> {
    // 快照磁盘读先于一切锁：本 handler 是面板轮询热点，不把 IO 关进临界区
    let snapshot = read_sub_nodes_snapshot(&state).await;
    let config = state.config.read().await;
    let status_map = state.sub_status.lock().await;
    // disabled_count 用生效口径：只统计匹配当前快照节点的条目；
    // 失配条目（如机场的「剩余流量」信息节点改名后）不产生效果，不应计入

    let source_by_url: HashMap<&str, String> = config
        .subs
        .iter()
        .map(|url| (url.as_str(), subscription_source_id(url)))
        .collect();
    let mut disabled_counts: HashMap<&str, usize> = HashMap::new();
    for entry in &config.disabled_nodes {
        if source_by_url
            .get(entry.sub.as_str())
            .and_then(|source| snapshot.as_ref()?.live_names.get(source))
            .is_some_and(|names| names.contains(&entry.name))
        {
            *disabled_counts.entry(&entry.sub).or_default() += 1;
        }
    }
    let subs_with_status: Vec<SubStatus> = config
        .subs
        .iter()
        .map(|url| {
            let mut status = status_map.get(url).cloned().unwrap_or(SubStatus {
                url: url.clone(),
                success: false,
                node_count: 0,
                disabled_count: 0,
                state: SubscriptionState::Pending,
                failure_kind: None,
                error: None,
            });
            status.disabled_count = disabled_counts.get(url.as_str()).copied().unwrap_or(0);
            status
        })
        .collect();

    success("Subscriptions loaded", subs_with_status)
}

/// 订阅详情弹窗的数据源：按订阅分组列出节点（含禁用标记）。
/// 纯读路径：数据来自 sub-nodes.json 快照，零网络；快照缺失的订阅返回空列表。
pub async fn get_sub_nodes(state: Arc<AppState>) -> CommandReply<Vec<SubNodesInfo>> {
    let config = state.config.read().await;
    let snapshot = read_sub_nodes_snapshot(&state).await;

    let disabled_keys: HashSet<(String, &str)> = config
        .disabled_nodes
        .iter()
        .map(|entry| (subscription_source_id(&entry.sub), entry.name.as_str()))
        .collect();

    let groups = config
        .subs
        .iter()
        .map(|url| {
            let source_id = subscription_source_id(url);
            let mut nodes = Vec::new();
            if let Some(snapshot) = &snapshot {
                for (index, entry_source) in snapshot.source_ids.iter().enumerate() {
                    if *entry_source != source_id {
                        continue;
                    }
                    let (Some(name), Some(outbound)) = (
                        snapshot.node_names.get(index),
                        snapshot.outbounds.get(index),
                    ) else {
                        continue;
                    };
                    if is_informational_subscription_node(name, outbound) {
                        continue;
                    }
                    let str_field = |key: &str| {
                        outbound
                            .get(key)
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                    };
                    nodes.push(SubNodeInfo {
                        name: name.clone(),
                        server: str_field("server").to_string(),
                        server_port: outbound
                            .get("server_port")
                            .and_then(|value| value.as_u64())
                            .unwrap_or_default() as u16,
                        node_type: str_field("type").to_string(),
                        disabled: disabled_keys.contains(&(source_id.clone(), name.as_str())),
                    });
                }
            }
            // 失配的禁用条目：存在禁用记录但当前快照里已没有同名节点
            let stale_disabled = config
                .disabled_nodes
                .iter()
                .filter(|entry| subscription_source_id(&entry.sub) == source_id)
                .filter(|entry| !nodes.iter().any(|node| node.name == entry.name))
                .map(|entry| entry.name.clone())
                .collect();
            SubNodesInfo {
                url: url.clone(),
                nodes,
                stale_disabled,
            }
        })
        .collect();

    success("Subscription nodes loaded", groups)
}

/// 禁用/启用订阅节点（易变层）。本地语义变更：快照零网络重建 + 热应用。
/// 成功路径会热重启内核；校验失败（订阅/节点不存在、禁用后空池）在事务前拦截。
pub async fn set_node_disabled(state: Arc<AppState>, req: SetNodeDisabledRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    let old_config = state.config.read().await.clone();
    // 锁外咨询性校验（快速 400 + 精确报错）；订阅存在性与空池校验在锁内闭包里
    // 还会基于最新配置权威重查——上锁间隙的并发变更不会逃过校验
    if !old_config.subs.contains(&req.sub) {
        return Err(command_error(CommandErrorKind::InvalidInput, "订阅不存在"));
    }
    // 存在性校验只约束「禁用」；「启用/清理」按名字移除条目即可，
    // 必须能清理已失配的条目（节点改名后快照里已不存在）
    let mut entries: Vec<(String, String)> = Vec::new();
    if req.disabled {
        let Some(snapshot) = read_sub_nodes_snapshot(&state).await else {
            return Err(command_error(
                CommandErrorKind::InvalidInput,
                "订阅节点尚未获取，无法设置禁用",
            ));
        };
        entries = snapshot
            .source_ids
            .iter()
            .zip(snapshot.node_names.iter())
            .enumerate()
            .filter_map(|(index, (source, name))| {
                let outbound = snapshot.outbounds.get(index)?;
                (!is_informational_subscription_node(name, outbound))
                    .then_some((source.clone(), name.clone()))
            })
            .collect();
        let source_id = subscription_source_id(&req.sub);
        if !entries.contains(&(source_id, req.name.clone())) {
            return Err(command_error(
                CommandErrorKind::InvalidInput,
                "订阅中不存在该节点",
            ));
        }
    }

    let sub = req.sub.clone();
    let name = req.name.clone();
    let disabled = req.disabled;
    let result = apply_disabled_nodes(&state, move |config| {
        if !config.subs.contains(&sub) {
            return Err("订阅不存在".to_string());
        }
        let mut next = config.disabled_nodes.clone();
        if disabled {
            if !next
                .iter()
                .any(|entry| entry.sub == sub && entry.name == name)
            {
                next.push(DisabledNode {
                    sub: sub.clone(),
                    name: name.clone(),
                });
            }
            // selector/urltest 不允许空 outbounds；禁用后可用池（订阅剩余 + 有效手动节点）
            // 为空会生成非法配置。锁内基于最新禁用集计算：并发禁用请求不会叠加出空池
            let disabled_keys: HashSet<(String, &str)> = next
                .iter()
                .map(|entry| (subscription_source_id(&entry.sub), entry.name.as_str()))
                .collect();
            let remaining = entries
                .iter()
                .filter(|(source, node_name)| {
                    !disabled_keys.contains(&(source.clone(), node_name.as_str()))
                })
                .count();
            let manual = collect_manual_outbounds(config).0.len();
            if remaining + manual == 0 {
                return Err("不能禁用全部节点".to_string());
            }
        } else {
            next.retain(|entry| !(entry.sub == sub && entry.name == name));
        }
        config.disabled_nodes = next;
        Ok(())
    })
    .await;

    match result {
        Ok(update) => Ok(success_no_data(if update.updated() {
            if req.disabled {
                "节点已禁用"
            } else {
                "节点已启用"
            }
        } else {
            "节点状态未变化"
        })),
        Err(ConfigMutationError::Superseded) => Err(command_error(
            CommandErrorKind::Conflict,
            "操作已被更新的请求取代",
        )),
        Err(ConfigMutationError::Rejected(message)) => {
            Err(command_error(CommandErrorKind::InvalidInput, message))
        }
        Err(ConfigMutationError::Apply(e)) => Err(command_error(CommandErrorKind::Internal, e)),
    }
}

pub async fn add_sub(state: Arc<AppState>, req: SubRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    if let Err(e) = Validator::subscription_url(&req.url) {
        return Err(command_error(CommandErrorKind::InvalidInput, e));
    }

    edit_subscriptions(&state, false, |subs| {
        if subs.contains(&req.url) {
            return Err("Subscription already exists".to_string());
        }
        subs.push(req.url);
        Ok(())
    })
    .await
    .map_err(|error| subscription_error(error, CommandErrorKind::InvalidInput))?;
    Ok(success_no_data("Subscription added"))
}

/// 扫描本机 clash-verge-rev 的订阅（只读）。未安装/无 remote 订阅时 found=false。
pub async fn get_verge_import(state: Arc<AppState>) -> CommandReply<VergeImportResult> {
    let subs = verge::scan().await.unwrap_or_default();
    let config = state.config.read().await;
    let items: Vec<VergeImportItem> = subs
        .into_iter()
        .map(|sub| VergeImportItem {
            already_added: config.subs.contains(&sub.url),
            name: sub.name,
            url: sub.url,
        })
        .collect();
    success(
        "Clash Verge subscriptions scanned",
        VergeImportResult {
            found: !items.is_empty(),
            items,
        },
    )
}

/// 批量添加订阅：与逐条调 add_sub 不同，全部 URL 在一次配置事务内提交，
/// 只触发一次生成/校验/热重载。已存在或批内重复的跳过并计数。
pub async fn add_subs_batch(
    state: Arc<AppState>,
    req: SubBatchRequest,
) -> CommandResult<SubBatchResult> {
    super::ensure_initialized(&state)?;

    let mut urls: Vec<String> = Vec::with_capacity(req.urls.len());
    for raw in &req.urls {
        let url = raw.trim().to_string();
        if let Err(e) = Validator::subscription_url(&url) {
            return Err(command_error(CommandErrorKind::InvalidInput, e));
        }
        if !urls.contains(&url) {
            urls.push(url);
        }
    }

    let (result, _) = edit_subscriptions(&state, false, |subs| {
        let mut added = 0;
        let mut skipped = 0;
        for url in urls {
            if subs.contains(&url) {
                skipped += 1;
            } else {
                subs.push(url);
                added += 1;
            }
        }
        Ok(SubBatchResult { added, skipped })
    })
    .await
    .map_err(|error| subscription_error(error, CommandErrorKind::InvalidInput))?;
    Ok(success(
        if result.added == 0 {
            "No new subscriptions to add"
        } else {
            "Subscriptions added"
        },
        result,
    ))
}

pub async fn delete_sub(state: Arc<AppState>, req: SubRequest) -> CommandResult {
    super::ensure_initialized(&state)?;

    edit_subscriptions(&state, false, |subs| {
        let previous = subs.len();
        subs.retain(|url| url != &req.url);
        if previous == subs.len() {
            return Err("Subscription not found".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|error| subscription_error(error, CommandErrorKind::NotFound))?;
    Ok(success_no_data("Subscription deleted"))
}

pub async fn refresh_subs(state: Arc<AppState>) -> CommandResult {
    super::ensure_initialized(&state)?;

    let update = refresh_subscriptions_foreground(&state)
        .await
        .map_err(|error| subscription_error(error, CommandErrorKind::InvalidInput))?;
    Ok(success_no_data(if update.updated() {
        "Subscriptions refreshed and runtime updated"
    } else {
        "Subscriptions refreshed"
    }))
}

fn subscription_error(
    error: ConfigMutationError,
    rejected: CommandErrorKind,
) -> super::CommandError {
    let status = match &error {
        ConfigMutationError::Rejected(_) => rejected,
        ConfigMutationError::Superseded => CommandErrorKind::Conflict,
        ConfigMutationError::Apply(_) => CommandErrorKind::Internal,
    };
    command_error(status, error)
}
