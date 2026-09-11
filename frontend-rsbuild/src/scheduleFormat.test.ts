import { describe, expect, it } from '@rstest/core'
import { formatNextRun, formatTimezoneLabel } from './scheduleFormat'

// 固定以 Asia/Tokyo（UTC+9）为后端时区断言；格式化显式传 timezone，
// 与本机时区、夏令时环境无关。
const TOKYO = 'Asia/Tokyo'
const NOW = '2026-09-11T12:00:00+09:00'

describe('formatTimezoneLabel', () => {
  it('shows the IANA name with the UTC offset', () => {
    expect(formatTimezoneLabel(TOKYO, '+09:00')).toBe('Asia/Tokyo（UTC+09:00）')
  })

  it('falls back to the offset when the name is unavailable', () => {
    expect(formatTimezoneLabel(undefined, '-05:00')).toBe('UTC-05:00')
  })
})

describe('formatNextRun', () => {
  it('labels same-day targets as 今天', () => {
    expect(formatNextRun('2026-09-11T20:00:00+09:00', TOKYO, NOW)).toBe('今天 20:00')
  })

  it('labels the next day as 明天', () => {
    expect(formatNextRun('2026-09-12T04:05:00+09:00', TOKYO, NOW)).toBe('明天 04:05')
  })

  it('falls back to a month/day label for later dates', () => {
    expect(formatNextRun('2026-09-15T04:05:00+09:00', TOKYO, NOW)).toBe('9月15日 04:05')
  })

  it('returns an empty string for missing or invalid input', () => {
    expect(formatNextRun(undefined, TOKYO, NOW)).toBe('')
    expect(formatNextRun('not-a-date', TOKYO, NOW)).toBe('')
  })

  it('formats with the backend timezone, not the browser timezone', () => {
    // 同一时刻在 UTC 下是 11:00：使用后端时区应显示 20:00
    expect(formatNextRun('2026-09-11T20:00:00+09:00', 'UTC', '2026-09-11T12:00:00+09:00'))
      .toBe('今天 11:00')
  })
})
