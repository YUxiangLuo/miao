use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::{atomic::Ordering, Arc};

use super::rules::build_rule_json;
use super::{
    command_error, success, success_no_data, CommandErrorKind, CommandReply, CommandResult,
};
use crate::models::{
    AssignTerminalRequest, DeleteTerminalRequest, RuleRequest, TerminalAssignMode, TerminalRecord,
    TerminalSummary, UpsertTerminalRequest,
};
use crate::services::config::{known_rule_targets, save_terminals, ConfigEdit};
use crate::services::dhcp;
use crate::state::AppState;

pub(crate) fn normalize_terminal_ip(raw: &str) -> Result<(Ipv4Addr, String), String> {
    let trimmed = raw.trim();
    let addr_part = trimmed.strip_suffix("/32").unwrap_or(trimmed);
    let ipv4 = addr_part
        .parse::<Ipv4Addr>()
        .map_err(|_| "请输入有效的局域网 IPv4 地址（如 192.168.8.180）".to_string())?;
    if ipv4.is_unspecified() || ipv4.is_broadcast() || ipv4.is_loopback() {
        return Err("终端地址不能是回环、未指定或广播地址".to_string());
    }
    Ok((ipv4, ipv4.to_string()))
}

fn clean_optional_text(
    raw: &Option<String>,
    max_chars: usize,
    label: &str,
) -> Result<Option<String>, String> {
    let Some(value) = raw.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > max_chars {
        return Err(format!("{label}不能超过 {max_chars} 个字符"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label}不能包含控制字符"));
    }
    Ok(Some(value.to_string()))
}

/// 若一条自定义规则是纯粹针对单个 IPv4 主机（`a.b.c.d` 或 `a.b.c.d/32`）的 `source_ip_cidr` 规则，提取该 IPv4。
pub(crate) fn extract_single_source_ipv4_rule(raw: &str) -> Option<Ipv4Addr> {
    let serde_json::Value::Object(map) = serde_json::from_str::<serde_json::Value>(raw).ok()?
    else {
        return None;
    };
    if map
        .keys()
        .any(|k| k != "source_ip_cidr" && k != "action" && k != "outbound")
    {
        return None;
    }
    let cidr_str = match map.get("source_ip_cidr")? {
        serde_json::Value::String(s) => s.as_str(),
        serde_json::Value::Array(arr) if arr.len() == 1 => arr[0].as_str()?,
        _ => return None,
    };
    let trimmed = cidr_str.trim();
    let ip_part = match trimmed.split_once('/') {
        Some((addr, "32")) => addr,
        None => trimmed,
        _ => return None,
    };
    let ipv4 = ip_part.parse::<Ipv4Addr>().ok()?;
    if ipv4.is_unspecified() || ipv4.is_broadcast() || ipv4.is_loopback() {
        return None;
    }
    Some(ipv4)
}

pub async fn get_terminals(state: Arc<AppState>) -> CommandReply<Vec<TerminalSummary>> {
    let saved = state.stable_config.read().await.terminals.clone();
    let custom_rules = state.config.read().await.custom_rules.clone();
    let leases = dhcp::discover_dhcp_leases().await;

    success(
        "Terminals loaded",
        merge_terminals(&saved, &leases, &custom_rules),
    )
}

pub(crate) fn merge_terminals(
    saved: &[TerminalRecord],
    leases: &[dhcp::DhcpLease],
    custom_rules: &[String],
) -> Vec<TerminalSummary> {
    let mut by_ip: BTreeMap<Ipv4Addr, TerminalSummary> = BTreeMap::new();

    for lease in leases {
        if let Ok(ipv4) = lease.ip.parse::<Ipv4Addr>() {
            by_ip.insert(
                ipv4,
                TerminalSummary {
                    ip: ipv4.to_string(),
                    name: None,
                    department: None,
                    note: None,
                    dhcp_hostname: lease.hostname.clone(),
                    mac: Some(lease.mac.clone()),
                },
            );
        }
    }

    for record in saved {
        if let Ok((ipv4, ip_str)) = normalize_terminal_ip(&record.ip) {
            let entry = by_ip.entry(ipv4).or_insert_with(|| TerminalSummary {
                ip: ip_str,
                name: None,
                department: None,
                note: None,
                dhcp_hostname: None,
                mac: None,
            });
            entry.name = Some(record.name.clone());
            entry.department = record.department.clone();
            entry.note = record.note.clone();
        }
    }

    for raw in custom_rules {
        if let Some(ipv4) = extract_single_source_ipv4_rule(raw) {
            by_ip.entry(ipv4).or_insert_with(|| TerminalSummary {
                ip: ipv4.to_string(),
                name: None,
                department: None,
                note: None,
                dhcp_hostname: None,
                mac: None,
            });
        }
    }

    by_ip.into_values().collect()
}

pub async fn upsert_terminal(
    state: Arc<AppState>,
    req: UpsertTerminalRequest,
) -> CommandResult<TerminalSummary> {
    super::ensure_initialized(&state)?;

    let (_ipv4, ip) = normalize_terminal_ip(&req.ip)
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "工位/终端名称不能为空",
        ));
    }
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(command_error(
            CommandErrorKind::InvalidInput,
            "工位/终端名称长度需在 1-64 个字符之间且不含控制字符",
        ));
    }
    let department = clean_optional_text(&req.department, 48, "所属部门")
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;
    let note = clean_optional_text(&req.note, 200, "备注说明")
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;

    let record = TerminalRecord {
        ip: ip.clone(),
        name: name.to_string(),
        department: department.clone(),
        note: note.clone(),
    };

    let _guard = state.config_update.lock().await;
    let mut terminals = state.stable_config.read().await.terminals.clone();
    if let Some(existing) = terminals.iter_mut().find(|item| item.ip == ip) {
        *existing = record;
    } else {
        terminals.push(record);
    }
    terminals.sort_by_key(|item| item.ip.parse::<Ipv4Addr>().unwrap_or(Ipv4Addr::UNSPECIFIED));

    save_terminals(&state, terminals)
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    state.data_revision.fetch_add(1, Ordering::Relaxed);

    let leases = dhcp::discover_dhcp_leases().await;
    let lease = leases.into_iter().find(|l| l.ip == ip);
    Ok(success(
        "Terminal saved",
        TerminalSummary {
            ip,
            name: Some(name.to_string()),
            department,
            note,
            dhcp_hostname: lease.as_ref().and_then(|l| l.hostname.clone()),
            mac: lease.map(|l| l.mac),
        },
    ))
}

pub async fn delete_terminal(state: Arc<AppState>, req: DeleteTerminalRequest) -> CommandResult {
    super::ensure_initialized(&state)?;
    let (_, ip) = normalize_terminal_ip(&req.ip)
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;

    let _guard = state.config_update.lock().await;
    let mut terminals = state.stable_config.read().await.terminals.clone();
    let before_len = terminals.len();
    terminals.retain(|item| item.ip != ip);
    if terminals.len() == before_len {
        return Err(command_error(
            CommandErrorKind::NotFound,
            "工位备注记录不存在",
        ));
    }

    save_terminals(&state, terminals)
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    state.data_revision.fetch_add(1, Ordering::Relaxed);
    Ok(success_no_data("Terminal deleted"))
}

/// 单事务原子切换某工位 IP 的整机路由策略（无漏流时间窗）。
pub async fn assign_terminal(state: Arc<AppState>, req: AssignTerminalRequest) -> CommandResult {
    super::ensure_initialized(&state)?;
    let (ipv4, ip_str) = normalize_terminal_ip(&req.ip)
        .map_err(|e| command_error(CommandErrorKind::InvalidInput, e))?;
    let cidr = format!("{ip_str}/32");

    let mut edit = ConfigEdit::begin(&state).await;

    let next_target: Option<String> = match req.mode {
        TerminalAssignMode::Pool => None,
        TerminalAssignMode::Direct => Some("direct".to_string()),
        TerminalAssignMode::BlockOverseas => Some("reject".to_string()),
        TerminalAssignMode::Dedicated => {
            let target = req
                .target
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    command_error(CommandErrorKind::InvalidInput, "请选择要锁定的独享专线节点")
                })?;
            if matches!(target, "proxy" | "direct" | "reject") {
                return Err(command_error(
                    CommandErrorKind::InvalidInput,
                    "锁定独享专线模式必须指定具体节点名称",
                ));
            }
            let known = known_rule_targets(edit.original(), &state).await;
            if !known.iter().any(|existing| existing == target) {
                return Err(command_error(
                    CommandErrorKind::InvalidInput,
                    format!("目标专线节点 '{}' 不存在", target),
                ));
            }
            Some(target.to_string())
        }
    };

    let next_rule_str = match next_target {
        Some(target) => {
            let rule_json = build_rule_json(&RuleRequest {
                field: "source_ip_cidr".to_string(),
                value: cidr,
                target,
            });
            Some(
                serde_json::to_string(&rule_json)
                    .map_err(|e| command_error(CommandErrorKind::Internal, e))?,
            )
        }
        None => None,
    };

    let matching_indices: Vec<usize> = edit
        .candidate
        .custom_rules
        .iter()
        .enumerate()
        .filter_map(|(idx, raw)| {
            (extract_single_source_ipv4_rule(raw) == Some(ipv4)).then_some(idx)
        })
        .collect();

    match next_rule_str {
        Some(rule_str) => {
            if let Some((&first, rest)) = matching_indices.split_first() {
                edit.candidate.custom_rules[first] = rule_str;
                for &idx in rest.iter().rev() {
                    edit.candidate.custom_rules.remove(idx);
                }
            } else {
                // 新工位整机规则置于前导工位规则段末尾、域名/端口业务规则之前，
                // 保持「大陆直连 -> 工位整机策略 -> 业务分流规则 -> 公共线路池」优先级。
                let insert_at = edit
                    .candidate
                    .custom_rules
                    .iter()
                    .take_while(|raw| extract_single_source_ipv4_rule(raw).is_some())
                    .count();
                edit.candidate.custom_rules.insert(insert_at, rule_str);
            }
        }
        None => {
            for &idx in matching_indices.iter().rev() {
                edit.candidate.custom_rules.remove(idx);
            }
        }
    }

    if edit.candidate.custom_rules == edit.original().custom_rules {
        return Ok(success_no_data("Terminal routing unchanged"));
    }

    edit.commit()
        .await
        .map_err(|e| command_error(CommandErrorKind::Internal, e))?;
    Ok(success_no_data("Terminal routing updated"))
}

#[cfg(test)]
mod tests {
    use super::merge_terminals;
    #[cfg(unix)]
    use super::{
        assign_terminal, delete_terminal, extract_single_source_ipv4_rule, upsert_terminal,
    };
    use crate::models::TerminalRecord;
    #[cfg(unix)]
    use crate::models::{
        AssignTerminalRequest, Config, DeleteTerminalRequest, TerminalAssignMode,
        UpsertTerminalRequest,
    };
    use crate::services::dhcp::DhcpLease;
    #[cfg(unix)]
    use crate::test_support::{install_fake_kernel, isolated_stopped_state};
    #[cfg(unix)]
    use std::net::Ipv4Addr;

    #[test]
    fn merges_dhcp_saved_records_and_rule_ips_sorted() {
        let saved = vec![TerminalRecord {
            ip: "192.168.8.214".to_string(),
            name: "张会计".to_string(),
            department: Some("财务部".to_string()),
            note: Some("仅国内网银".to_string()),
        }];
        let leases = vec![
            DhcpLease {
                ip: "192.168.8.214".to_string(),
                mac: "00:e0:4d:a4:0c:35".to_string(),
                hostname: Some("CAIWU".to_string()),
            },
            DhcpLease {
                ip: "192.168.8.180".to_string(),
                mac: "80:c5:f2:2b:e3:6c".to_string(),
                hostname: Some("YY".to_string()),
            },
        ];
        let rules = vec![
            r#"{"source_ip_cidr":"192.168.8.50/32","action":"route","outbound":"direct"}"#
                .to_string(),
        ];

        let merged = merge_terminals(&saved, &leases, &rules);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].ip, "192.168.8.50");
        assert_eq!(merged[1].ip, "192.168.8.180");
        assert_eq!(merged[1].dhcp_hostname.as_deref(), Some("YY"));
        assert_eq!(merged[2].ip, "192.168.8.214");
        assert_eq!(merged[2].name.as_deref(), Some("张会计"));
        assert_eq!(merged[2].department.as_deref(), Some("财务部"));
        assert_eq!(merged[2].dhcp_hostname.as_deref(), Some("CAIWU"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn upsert_delete_and_atomic_assign_terminal_preserve_priority_order() {
        let (_root, state) = isolated_stopped_state(Config {
            nodes: vec![
                r#"{"type":"hysteria2","tag":"us-dedicated","server":"us.example.com","server_port":443,"password":"secret-password","tls":{"enabled":true}}"#.to_string(),
            ],
            custom_rules: vec![
                r#"{"domain_suffix":"whatsapp.net","action":"route","outbound":"proxy"}"#
                    .to_string(),
            ],
            ..Default::default()
        });
        install_fake_kernel(&state);

        let saved = upsert_terminal(
            state.clone(),
            UpsertTerminalRequest {
                ip: "192.168.8.180".to_string(),
                name: "李运营".to_string(),
                department: Some("美区组".to_string()),
                note: None,
            },
        )
        .await
        .unwrap()
        .data
        .unwrap();
        assert_eq!(saved.name.as_deref(), Some("李运营"));

        // Assign dedicated line: inserted BEFORE domain_suffix rule
        assign_terminal(
            state.clone(),
            AssignTerminalRequest {
                ip: "192.168.8.180".to_string(),
                mode: TerminalAssignMode::Dedicated,
                target: Some("us-dedicated".to_string()),
            },
        )
        .await
        .unwrap();
        {
            let rules = &state.config.read().await.custom_rules;
            assert_eq!(rules.len(), 2);
            assert_eq!(
                extract_single_source_ipv4_rule(&rules[0]),
                Some("192.168.8.180".parse::<Ipv4Addr>().unwrap())
            );
            assert!(rules[0].contains("us-dedicated"));
            assert!(rules[1].contains("whatsapp.net"));
        }

        // Switch atomically to block_overseas (reject) in-place
        assign_terminal(
            state.clone(),
            AssignTerminalRequest {
                ip: "192.168.8.180".to_string(),
                mode: TerminalAssignMode::BlockOverseas,
                target: None,
            },
        )
        .await
        .unwrap();
        {
            let rules = &state.config.read().await.custom_rules;
            assert_eq!(rules.len(), 2);
            assert!(rules[0].contains(r#""action":"reject""#));
        }

        // Switch back to shared pool: removes the seat rule cleanly
        assign_terminal(
            state.clone(),
            AssignTerminalRequest {
                ip: "192.168.8.180".to_string(),
                mode: TerminalAssignMode::Pool,
                target: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(state.config.read().await.custom_rules.len(), 1);

        delete_terminal(
            state,
            DeleteTerminalRequest {
                ip: "192.168.8.180".to_string(),
            },
        )
        .await
        .unwrap();
    }
}
