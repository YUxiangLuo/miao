//! 定时刷新订阅的纯时间计算：时刻按运行主机的系统本地时区解释。
//! 持久配置是 [`ScheduledRefresh`]，实际调度循环在 `runtime::scheduler`。
use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveTime, SecondsFormat, TimeZone};

use crate::models::{ScheduledRefresh, ScheduledRefreshStatus};

pub const TIME_FORMAT: &str = "%H:%M";
/// 一天内的时刻上限：再多也失去「定时」意义，同时避免面板误提交超长列表。
pub const MAX_TIMES: usize = 24;

/// 校验并规范化用户提交的时刻：非法格式报错；输出按时间排序、去重。
pub fn normalize_times(raw: &[String]) -> Result<Vec<String>, String> {
    if raw.len() > MAX_TIMES {
        return Err(format!("最多只能设置 {MAX_TIMES} 个时间点"));
    }
    let mut parsed = Vec::with_capacity(raw.len());
    for value in raw {
        parsed.push(parse_time(value)?);
    }
    parsed.sort_unstable();
    parsed.dedup();
    Ok(parsed.into_iter().map(format_time).collect())
}

pub fn parse_time(value: &str) -> Result<NaiveTime, String> {
    NaiveTime::parse_from_str(value.trim(), TIME_FORMAT)
        .map_err(|_| format!("无效的时间 {value:?}，应为 HH:MM"))
}

pub fn format_time(time: NaiveTime) -> String {
    time.format(TIME_FORMAT).to_string()
}

/// 解析持久化列表；非法条目忽略（config.yaml 可被手改，调度不应因此崩溃）。
pub fn parse_times(values: &[String]) -> Vec<NaiveTime> {
    values
        .iter()
        .filter_map(|value| parse_time(value).ok())
        .collect()
}

/// 严格晚于 `now` 的下一次执行时刻；没有有效时刻时为 None。
/// 夏令时跳变中不存在的本地时刻（`LocalResult::None`）跳过当天。
pub fn next_run_at(times: &[NaiveTime], now: DateTime<Local>) -> Option<DateTime<Local>> {
    let today = now.date_naive();
    let tomorrow = today.succ_opt()?;
    times
        .iter()
        .filter_map(|time| {
            [today, tomorrow]
                .into_iter()
                .find_map(|date| resolve_local(date, *time).filter(|candidate| *candidate > now))
        })
        .min()
}

fn resolve_local(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    match Local.from_local_datetime(&date.and_time(time)) {
        LocalResult::Single(value) => Some(value),
        // 回拨重叠：取第一次出现，调度循环的「已到达目标」判断不会重复触发。
        LocalResult::Ambiguous(earliest, _) => Some(earliest),
        LocalResult::None => None,
    }
}

/// 当前系统时区 IANA 名；精简容器等环境可能拿不到，此时只展示 UTC 偏移。
pub fn timezone_name() -> Option<String> {
    iana_time_zone::get_timezone().ok()
}

/// REST/MCP 共用的状态投影：`next_run_at` 每次按当前时间重算。
/// 时间统一输出到秒：`to_rfc3339()` 的纳秒小数在部分浏览器的 `Date.parse` 下不可靠。
pub fn status(schedule: &ScheduledRefresh) -> ScheduledRefreshStatus {
    let now = Local::now();
    let times = parse_times(&schedule.times);
    ScheduledRefreshStatus {
        enabled: schedule.enabled,
        times: schedule.times.clone(),
        timezone: timezone_name(),
        utc_offset: now.offset().to_string(),
        now: now.to_rfc3339_opts(SecondsFormat::Secs, false),
        next_run_at: schedule
            .enabled
            .then(|| next_run_at(&times, now))
            .flatten()
            .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .earliest()
            .expect("test local time is constructible")
    }

    fn times(values: &[&str]) -> Vec<NaiveTime> {
        values
            .iter()
            .map(|value| parse_time(value).unwrap())
            .collect()
    }

    #[test]
    fn normalization_sorts_dedupes_and_canonicalizes() {
        let raw: Vec<String> = ["16:05", "4:5", "16:05", "00:00"]
            .iter()
            .map(|value| value.to_string())
            .collect();
        assert_eq!(
            normalize_times(&raw).unwrap(),
            vec!["00:00", "04:05", "16:05"]
        );
    }

    #[test]
    fn normalization_rejects_invalid_and_oversized_input() {
        assert!(normalize_times(&["25:00".into()]).is_err());
        assert!(normalize_times(&["aa:bb".into()]).is_err());
        let oversized: Vec<String> = (0..=MAX_TIMES).map(|_| "01:00".to_string()).collect();
        assert!(normalize_times(&oversized).is_err());
    }

    #[test]
    fn persisted_invalid_entries_are_ignored() {
        assert_eq!(
            parse_times(&["08:30".into(), "oops".into()]),
            times(&["08:30"])
        );
    }

    #[test]
    fn next_run_prefers_today_then_rolls_over_to_tomorrow() {
        let now = local(2026, 9, 11, 12, 0);
        assert_eq!(
            next_run_at(&times(&["04:00", "15:00", "23:59"]), now),
            Some(local(2026, 9, 11, 15, 0))
        );
        // 当天时刻已过 → 明天最近的时刻
        assert_eq!(
            next_run_at(&times(&["09:00"]), now),
            Some(local(2026, 9, 12, 9, 0))
        );
    }

    #[test]
    fn next_run_returns_none_without_times() {
        assert_eq!(next_run_at(&[], local(2026, 9, 11, 12, 0)), None);
    }

    #[test]
    fn status_keeps_stored_times_and_omits_next_run_when_disabled() {
        let schedule = ScheduledRefresh {
            enabled: false,
            times: vec!["08:05".into(), "20:00".into()],
        };
        let disabled_status = status(&schedule);
        assert!(!disabled_status.enabled);
        assert_eq!(disabled_status.times, vec!["08:05", "20:00"]);
        assert!(disabled_status.next_run_at.is_none());
        assert!(!disabled_status.utc_offset.is_empty());
        assert!(chrono::DateTime::parse_from_rfc3339(&disabled_status.now).is_ok());

        let enabled = ScheduledRefresh {
            enabled: true,
            times: vec!["08:05".into()],
        };
        let enabled_status = status(&enabled);
        let next = enabled_status
            .next_run_at
            .expect("enabled schedule has a next run");
        assert!(chrono::DateTime::parse_from_rfc3339(&next).is_ok());
    }
}
