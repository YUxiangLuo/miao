// 定时刷新弹窗的时间展示。
// 执行时刻始终按后端系统时区解释（timezone/utc_offset 来自 API），
// 这里不能使用浏览器本地时区格式化——远程打开面板时两者可能不同。

/** "Asia/Tokyo（UTC+09:00）"；时区名不可得时退化为 "UTC+09:00"。 */
export function formatTimezoneLabel(timezone: string | undefined, utcOffset: string): string {
  return timezone ? `${timezone}（UTC${utcOffset}）` : `UTC${utcOffset}`
}

/** 在指定时区下的日期键（en-CA 输出 YYYY-MM-DD，可直接比较）。 */
function dateKey(date: Date, timeZone: string | undefined): string {
  return new Intl.DateTimeFormat('en-CA', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).format(date)
}

function timeLabel(date: Date, timeZone: string | undefined): string {
  return new Intl.DateTimeFormat('zh-CN', {
    timeZone,
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).format(date)
}

/** 月/日文案手动拼接：部分运行时（Bun/JSC）的 zh-CN 数字日期格式是 "9/15"。 */
function monthDayLabel(date: Date, timeZone: string | undefined): string {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone,
    month: 'numeric',
    day: 'numeric',
  }).formatToParts(date)
  const month = parts.find((part) => part.type === 'month')?.value ?? ''
  const day = parts.find((part) => part.type === 'day')?.value ?? ''
  return `${month}月${day}日`
}

/**
 * 下次执行时间的可读文案："今天 15:00" / "明天 03:00" / "9月15日 03:00"。
 * 无法解析时返回空字符串，由调用方决定是否展示。
 */
export function formatNextRun(
  nextRunAt: string | undefined,
  timezone: string | undefined,
  now: string,
): string {
  if (!nextRunAt) return ''
  const next = new Date(nextRunAt)
  if (Number.isNaN(next.getTime())) return ''
  const time = timeLabel(next, timezone)

  const current = new Date(now)
  if (Number.isNaN(current.getTime())) {
    return `${monthDayLabel(next, timezone)} ${time}`
  }

  const nextKey = dateKey(next, timezone)
  if (nextKey === dateKey(current, timezone)) return `今天 ${time}`
  // 目标时区下的“明天”：按墙钟加 24h 只用于日期比较；夏令时切换日可能偏一天，
  // 但仅影响文案（今天/明天/日期），不影响实际执行时刻。
  const tomorrow = new Date(current.getTime() + 24 * 60 * 60 * 1000)
  if (nextKey === dateKey(tomorrow, timezone)) return `明天 ${time}`
  return `${monthDayLabel(next, timezone)} ${time}`
}
