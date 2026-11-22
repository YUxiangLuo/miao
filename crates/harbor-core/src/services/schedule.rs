//! 定时刷新订阅的纯时间计算：时刻按运行主机的系统本地时区解释。
//! 持久配置是 [`ScheduledRefresh`]，实际调度循环在 `runtime::scheduler`。
use jiff::{
    civil::{Date, Time},
    tz::{AmbiguousOffset, TimeZone},
    Timestamp, ZonedDateTime,
};

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

pub fn parse_time(value: &str) -> Result<Time, String> {
    let err = || format!("无效的时间 {value:?}，应为 HH:MM");
    let Some((hour, minute)) = value.trim().split_once(':') else {
        return Err(err());
    };
    if !(1..=2).contains(&hour.len())
        || !(1..=2).contains(&minute.len())
        || !hour.bytes().all(|b| b.is_ascii_digit())
        || !minute.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(err());
    }
    let hour: i8 = hour.parse().map_err(|_| err())?;
    let minute: i8 = minute.parse().map_err(|_| err())?;
    Time::new(hour, minute, 0, 0).map_err(|_| err())
}

pub fn format_time(time: Time) -> String {
    time.strftime(TIME_FORMAT).to_string()
}

/// 解析持久化列表；非法条目忽略（config.yaml 可被手改，调度不应因此崩溃）。
pub fn parse_times(values: &[String]) -> Vec<Time> {
    values
        .iter()
        .filter_map(|value| parse_time(value).ok())
        .collect()
}

fn local_time_zone() -> TimeZone {
    TimeZone::try_system()
        .ok()
        .or_else(|| {
            let name = iana_time_zone::get_timezone().ok()?;
            jiff::tz::db().get(&name).ok()
        })
        .or_else(|| {
            let posix = std::fs::read_to_string("/etc/TZ").ok()?;
            TimeZone::posix(posix.trim()).ok()
        })
        .unwrap_or(TimeZone::UTC)
}

pub fn local_now() -> ZonedDateTime {
    ZonedDateTime::new(Timestamp::now(), local_time_zone())
}

/// 严格晚于 `now` 的下一次执行时刻；没有有效时刻时为 None。
/// 夏令时跳变中不存在的本地时刻跳过当天。
pub fn next_run_at(times: &[Time], now: &ZonedDateTime) -> Option<ZonedDateTime> {
    let today = now.date();
    let tomorrow = today.tomorrow().ok()?;
    let tz = now.time_zone();
    times
        .iter()
        .filter_map(|time| {
            [today, tomorrow]
                .into_iter()
                .find_map(|date| resolve_local(date, *time, tz).filter(|candidate| candidate > now))
        })
        .min()
}

fn resolve_local(date: Date, time: Time, tz: &TimeZone) -> Option<ZonedDateTime> {
    let dt = date.to_datetime(time);
    let offset = match tz.to_ambiguous_timestamp(dt).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset,
        // 回拨重叠：取第一次出现，调度循环的「已到达目标」判断不会重复触发。
        AmbiguousOffset::Fold { before, .. } => before,
        AmbiguousOffset::Gap { .. } => return None,
    };
    Some(ZonedDateTime::new(
        offset.to_timestamp(dt).ok()?,
        tz.clone(),
    ))
}

/// 当前系统时区 IANA 名；精简容器等环境可能拿不到，此时只展示 UTC 偏移。
pub fn timezone_name() -> Option<String> {
    TimeZone::try_system()
        .ok()
        .and_then(|tz| tz.iana_name().map(str::to_owned))
        .or_else(|| iana_time_zone::get_timezone().ok())
}

fn format_rfc3339_secs(at: &ZonedDateTime) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// REST/MCP 共用的状态投影：`next_run_at` 每次按当前时间重算。
/// 时间统一输出到秒：纳秒小数在部分浏览器的 `Date.parse` 下不可靠。
pub fn status(schedule: &ScheduledRefresh) -> ScheduledRefreshStatus {
    let now = local_now();
    let times = parse_times(&schedule.times);
    ScheduledRefreshStatus {
        enabled: schedule.enabled,
        times: schedule.times.clone(),
        timezone: timezone_name(),
        utc_offset: now.strftime("%:z").to_string(),
        now: format_rfc3339_secs(&now),
        next_run_at: schedule
            .enabled
            .then(|| next_run_at(&times, &now))
            .flatten()
            .map(|at| format_rfc3339_secs(&at)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(y: i16, m: i8, d: i8, h: i8, min: i8) -> ZonedDateTime {
        let date = Date::new(y, m, d).expect("valid test date");
        let time = Time::new(h, min, 0, 0).expect("valid test time");
        resolve_local(date, time, &local_time_zone()).expect("test local time is constructible")
    }

    fn times(values: &[&str]) -> Vec<Time> {
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
        assert!(normalize_times(&["+4:05".into()]).is_err());
        assert!(normalize_times(&["12:00:00".into()]).is_err());
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
            next_run_at(&times(&["04:00", "15:00", "23:59"]), &now),
            Some(local(2026, 9, 11, 15, 0))
        );
        // 当天时刻已过 → 明天最近的时刻
        assert_eq!(
            next_run_at(&times(&["09:00"]), &now),
            Some(local(2026, 9, 12, 9, 0))
        );
    }

    #[test]
    fn dst_gap_skips_nonexistent_today_and_fold_picks_first_occurrence() {
        let Ok(ny) = jiff::tz::db().get("America/New_York") else {
            return;
        };
        // 2026-03-08 02:30 在纽约夏令时跳变中不存在，应跳过当天落到次日 02:30 (-04:00)
        let before_gap = resolve_local(
            Date::new(2026, 3, 8).unwrap(),
            Time::new(1, 0, 0, 0).unwrap(),
            &ny,
        )
        .unwrap();
        let next = next_run_at(&times(&["02:30"]), &before_gap).unwrap();
        assert_eq!(format_rfc3339_secs(&next), "2026-03-09T02:30:00-04:00");

        // 2026-11-01 01:30 在纽约回拨中出现两次，应取第一次 (-04:00)
        let before_fold = resolve_local(
            Date::new(2026, 11, 1).unwrap(),
            Time::new(0, 30, 0, 0).unwrap(),
            &ny,
        )
        .unwrap();
        let fold_next = next_run_at(&times(&["01:30"]), &before_fold).unwrap();
        assert_eq!(format_rfc3339_secs(&fold_next), "2026-11-01T01:30:00-04:00");
    }

    #[test]
    fn next_run_returns_none_without_times() {
        assert_eq!(next_run_at(&[], &local(2026, 9, 11, 12, 0)), None);
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
        assert!(disabled_status.now.parse::<Timestamp>().is_ok());

        let enabled = ScheduledRefresh {
            enabled: true,
            times: vec!["08:05".into()],
        };
        let enabled_status = status(&enabled);
        let next = enabled_status
            .next_run_at
            .expect("enabled schedule has a next run");
        assert!(next.parse::<Timestamp>().is_ok());
    }
}
