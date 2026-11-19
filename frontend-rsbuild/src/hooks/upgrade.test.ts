import { afterEach, expect, it, rs } from '@rstest/core'
import {
  classifyUpgradeTimeoutError,
  describeVersionCheckResult,
  UPGRADE_RESTART_TIMEOUT_MS,
  versionsMatch,
  waitForUpgrade,
} from './upgrade'

afterEach(() => { rs.useRealTimers(); rs.unstubAllGlobals() })

it('waits for the exact target, allowing the release v prefix', async () => {
  rs.useFakeTimers()
  const fetchMock = rs.fn()
    .mockResolvedValueOnce({ ok: true, json: async () => ({ success: true, data: { current: '0.47.0' } }) })
    .mockResolvedValue({ ok: true, json: async () => ({ success: true, data: { current: '0.49.0' } }) })
  rs.stubGlobal('fetch', fetchMock)
  let done = false
  const result = waitForUpgrade('v0.49.0', '0.48.1').then(() => { done = true })
  await rs.advanceTimersByTimeAsync(500)
  expect(done).toBe(false)
  await rs.advanceTimersByTimeAsync(500)
  await result
  expect(done).toBe(true)
})

it('enforces the total deadline even when every version fetch hangs', async () => {
  rs.useFakeTimers()
  rs.stubGlobal('fetch', rs.fn(() => new Promise(() => {})))
  const result = waitForUpgrade('0.49.0', '0.48.1').catch(error => error)
  await rs.advanceTimersByTimeAsync(UPGRADE_RESTART_TIMEOUT_MS)
  expect((await result as Error).message).toContain('升级结果未知')
})

it.each(['0.48.1', 'v0.48.1'])('does not claim success or certain rollback when the old version remains: %s', async (previous) => {
  rs.useFakeTimers()
  rs.stubGlobal('fetch', rs.fn(async () => ({ ok: true, json: async () => ({ success: true, data: { current: '0.48.1' } }) })))
  const result = waitForUpgrade('0.49.0', previous).catch(error => error)
  await rs.advanceTimersByTimeAsync(UPGRADE_RESTART_TIMEOUT_MS)
  expect((await result as Error).message).toContain('可能尚未重启或已回滚')
})

it('classifies version matching, timeout states, and manual version checks', () => {
  expect(versionsMatch('v0.49.0', '0.49.0')).toBe(true)
  expect(versionsMatch('0.49.0', '0.49.1')).toBe(false)

  expect(classifyUpgradeTimeoutError(undefined, '0.48.1')).toContain('升级结果未知')
  expect(classifyUpgradeTimeoutError('0.48.1', 'v0.48.1')).toContain('可能尚未重启或已回滚')
  expect(classifyUpgradeTimeoutError('0.49.0', '0.48.1')).toContain('当前版本为 0.49.0')

  expect(describeVersionCheckResult({ current: '0.48.1', latest: 'v0.49.0', has_update: true, download_url: null, upgrade_supported: true }))
    .toEqual({ message: '发现新版本 v0.49.0', tone: 'success' })
  expect(describeVersionCheckResult({ current: '0.48.1', latest: null, has_update: false, download_url: null, upgrade_supported: true }))
    .toEqual({ message: '暂未获取到最新版本信息，请稍后重试', tone: 'info' })
  expect(describeVersionCheckResult({ current: '0.48.1', latest: '0.48.1', has_update: false, download_url: null, upgrade_supported: true }))
    .toEqual({ message: '当前已是最新版本', tone: 'info' })
})
