import { useCallback, useMemo, useState } from 'react'
import { fetchJson } from './request'
import { useLatestRequest } from './useLatestRequest'
import type { ApiResponse, NodeInfo, RuleInfo, StatusData, SubStatus, VersionInfo } from '../types/api'

export type ResourceReadResult<T> =
  | { status: 'success'; data: T }
  | { status: 'error'; error: string }
  | { status: 'superseded' }

// 未拿到首个响应前的占位状态。vps_supported: true 与历史上的 undefined 等价
// 消费方判断 `!== false`，保留旧 API 的默认能力值。
const INITIAL_STATUS: StatusData = {
  data_revision: 0,
  subscription_refresh: {
    phase: 'idle', outcome: 'not_requested', retry_in_secs: null,
    report: { successful_sources: 0, failed_sources: 0, fresh_nodes: 0, cached_nodes: 0 },
  },
  running: false,
  ready: false,
  phase: 'initializing',
  initializing: false,
  route_mode: 'rule',
  node_select: 'manual',
  requested_node_select: 'manual',
  max_multiplier: null,
  multiplier_options: [],
  vps_supported: true,
  platform: 'linux',
  warnings: [],
  mcp: false,
}

function useResource<T>(path: string, initial: T) {
  const [data, setData] = useState(initial)
  const [loaded, setLoaded] = useState(false)
  const [settled, setSettled] = useState(false)
  const [failures, setFailures] = useState(0)
  const [error, setError] = useState('')
  const { begin } = useLatestRequest()
  const refresh = useCallback(async (): Promise<ResourceReadResult<T>> => {
    const request = begin()
    try {
      const payload = await fetchJson<ApiResponse<T>>(path, { signal: request.signal })
      if (!request.isCurrent()) return { status: 'superseded' }
      if (!payload.success || payload.data == null) throw new Error(payload.message || '请求失败')
      const next = payload.data
      // Stable references let memoized rules and lists survive health polling.
      setData(previous => JSON.stringify(previous) === JSON.stringify(next) ? previous : next)
      setLoaded(true)
      setFailures(0)
      setError('')
      return { status: 'success', data: next }
    } catch (cause) {
      if (!request.isCurrent()) return { status: 'superseded' }
      const message = cause instanceof Error ? cause.message : String(cause)
      setFailures(count => count + 1)
      setError(message)
      return { status: 'error', error: message }
    } finally {
      if (request.isCurrent()) setSettled(true)
    }
  }, [path, begin])
  return { data, loaded, settled, failures, error, refresh }
}

export function useStatus() {
  const resource = useResource('/api/status', INITIAL_STATUS)
  const status = useMemo(() => {
    const value = resource.data
    const ready = value.ready ?? (value.running && !value.initializing)
    return { ...value, ready, phase: value.phase ?? (ready ? 'ready' : value.initializing ? 'initializing' : 'stopped') }
  }, [resource.data])
  return {
    status,
    statusLoaded: resource.loaded,
    statusSettled: resource.settled,
    statusFailures: resource.failures,
    fetchStatus: resource.refresh,
  }
}

export function useSubs() {
  const resource = useResource<SubStatus[]>('/api/subs', [])
  return { subs: resource.data, subsLoaded: resource.settled, subsAvailable: resource.loaded, subsError: resource.error, fetchSubs: resource.refresh }
}

export function useNodes() {
  const resource = useResource<NodeInfo[]>('/api/nodes', [])
  return { nodes: resource.data, nodesLoaded: resource.settled, nodesAvailable: resource.loaded, nodesError: resource.error, fetchNodes: resource.refresh }
}

export function useRules() {
  const resource = useResource<RuleInfo[]>('/api/rules', [])
  return { rules: resource.data, rulesLoaded: resource.settled, rulesError: resource.error, fetchRules: resource.refresh }
}

const INITIAL_VERSION: VersionInfo = {
  current: '',
  latest: null,
  has_update: false,
  download_url: null,
  upgrade_supported: true,
}

export function useVersion() {
  const resource = useResource('/api/version', INITIAL_VERSION)
  return { versionInfo: resource.data, fetchVersion: resource.refresh }
}
