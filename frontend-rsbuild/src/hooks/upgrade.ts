import type { ApiResponse, VersionInfo } from '../types/api'
import type { ToastTone } from './useApi'
import { fetchJson } from './request'

export const UPGRADE_RESTART_TIMEOUT_MS = 30_000
const UPGRADE_POLL_INTERVAL_MS = 500
const UPGRADE_PROBE_TIMEOUT_MS = 2_000

export function normalizeVersionTag(version: string): string {
  return version.replace(/^v/, '')
}

export function versionsMatch(left: string, right: string): boolean {
  return normalizeVersionTag(left) === normalizeVersionTag(right)
}

export function describeVersionCheckResult(fresh: VersionInfo | undefined): { message: string; tone: ToastTone } {
  if (fresh?.has_update) {
    return { message: `发现新版本 ${fresh.latest}`, tone: 'success' }
  }
  if (!fresh?.latest) {
    return { message: '暂未获取到最新版本信息，请稍后重试', tone: 'info' }
  }
  return { message: '当前已是最新版本', tone: 'info' }
}

export function classifyUpgradeTimeoutError(observed: string | undefined, previous: string): string {
  if (observed !== undefined && versionsMatch(observed, previous)) {
    return '尚未确认目标版本，服务仍报告旧版本（可能尚未重启或已回滚）；请刷新状态确认，勿重复升级'
  }
  if (observed) {
    return `尚未确认目标版本，当前版本为 ${observed}；请刷新状态确认`
  }
  return '等待服务重启超时，升级结果未知；请手动刷新页面确认'
}

// Confirm the version, not data-plane health: an installed version can still
// be initializing. Never equate a different (possibly rolled-back) version
// with the requested release.
export async function waitForUpgrade(target: string, previous: string): Promise<void> {
  const deadline = Date.now() + UPGRADE_RESTART_TIMEOUT_MS
  let observed: string | undefined

  while (Date.now() < deadline) {
    const sleepMs = Math.min(UPGRADE_POLL_INTERVAL_MS, deadline - Date.now())
    await new Promise(resolve => window.setTimeout(resolve, sleepMs))

    const remaining = deadline - Date.now()
    if (remaining <= 0) break

    try {
      const payload = await fetchJson<ApiResponse<VersionInfo>>(
        '/api/version',
        {},
        Math.min(UPGRADE_PROBE_TIMEOUT_MS, remaining),
      )
      if (!payload.success || !payload.data?.current) continue
      observed = payload.data.current
      if (versionsMatch(observed, target)) return
    } catch {
      // A restart temporarily makes the endpoint unreachable.
    }
  }

  throw new Error(classifyUpgradeTimeoutError(observed, previous))
}
