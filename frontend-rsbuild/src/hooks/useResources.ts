import { createMemo, createSignal, untrack, type Accessor } from 'solid-js'
import { fetchJson, HttpStatusError } from './request'
import { useLatestRequest } from './useLatestRequest'
import type {
  AccessPortStatus,
  ApiResponse,
  NodeInfo,
  RuleInfo,
  StatusData,
  SubStatus,
  TerminalSummary,
  VersionInfo,
} from '../types/api'

export type ResourceReadResult<T> =
  | { status: 'success'; data: T }
  | { status: 'error'; error: string }
  | { status: 'superseded' }

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

function isSameResourceValue<T>(previous: T, next: T): boolean {
  return JSON.stringify(previous) === JSON.stringify(next)
}

export function normalizeStatusData(value: StatusData): StatusData {
  const ready = value.ready ?? (value.running && !value.initializing)
  const fallbackPhase = ready ? 'ready' : value.initializing ? 'initializing' : 'stopped'
  return { ...value, ready, phase: value.phase ?? fallbackPhase }
}

function useResource<T>(path: string, initial: T, revision: Accessor<number> = () => 0) {
  const [box, setBox] = createSignal({ value: initial })
  const data: Accessor<T> = () => box().value
  const [readRevision, setReadRevision] = createSignal<number | undefined>(undefined)
  const [loaded, setLoaded] = createSignal(false)
  const [settled, setSettled] = createSignal(false)
  const [failures, setFailures] = createSignal(0)
  const [error, setError] = createSignal('')
  const { begin } = useLatestRequest()

  const refresh = async (): Promise<ResourceReadResult<T>> => {
    const requestRevision = untrack(revision)
    const request = begin()
    try {
      const payload = await fetchJson<ApiResponse<T>>(path, { signal: request.signal })
      if (!request.isCurrent()) return { status: 'superseded' }
      if (!payload.success || payload.data == null) throw new Error(payload.message || '请求失败')
      const next = payload.data
      setBox((previous) => (isSameResourceValue(previous.value, next) ? previous : { value: next }))
      setLoaded(true)
      setReadRevision(requestRevision)
      setFailures(0)
      setError('')
      return { status: 'success', data: next }
    } catch (cause) {
      if (!request.isCurrent()) return { status: 'superseded' }
      const message = cause instanceof Error ? cause.message : String(cause)
      setFailures((count) => count + 1)
      setError(message)
      return { status: 'error', error: message }
    } finally {
      if (request.isCurrent()) setSettled(true)
    }
  }

  const current = createMemo(() => readRevision() === revision())

  return { data, loaded, settled, failures, error, refresh, current }
}

export function useStatus() {
  const resource = useResource('/api/status', INITIAL_STATUS)
  const status = createMemo(() => normalizeStatusData(resource.data()))
  return {
    status,
    statusLoaded: resource.loaded,
    statusSettled: resource.settled,
    statusFailures: resource.failures,
    fetchStatus: resource.refresh,
  }
}

export function useSubs(revision: Accessor<number> = () => 0) {
  const resource = useResource<SubStatus[]>('/api/subs', [], revision)
  return {
    subs: resource.data,
    subsLoaded: resource.settled,
    subsAvailable: resource.loaded,
    subsCurrent: resource.current,
    subsError: resource.error,
    fetchSubs: resource.refresh,
  }
}

export function useNodes(revision: Accessor<number> = () => 0) {
  const resource = useResource<NodeInfo[]>('/api/nodes', [], revision)
  return {
    nodes: resource.data,
    nodesLoaded: resource.settled,
    nodesAvailable: resource.loaded,
    nodesCurrent: resource.current,
    nodesError: resource.error,
    fetchNodes: resource.refresh,
  }
}

export function useRules(revision: Accessor<number> = () => 0) {
  const resource = useResource<RuleInfo[]>('/api/rules', [], revision)
  return {
    rules: resource.data,
    rulesLoaded: resource.settled,
    rulesCurrent: resource.current,
    rulesError: resource.error,
    fetchRules: resource.refresh,
  }
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

/**
 * 兼容旧版网关二进制的可选资源读取：若远端返回 HTTP 404（尚未升级支持新端点），
 * 标记 supported=false 并降级为空数组，不向界面抛出红色报错横幅。
 */
function useOptionalResource<T>(path: string, initial: T, revision: Accessor<number> = () => 0) {
  const [box, setBox] = createSignal({ value: initial })
  const data: Accessor<T> = () => box().value
  const [supported, setSupported] = createSignal(true)
  const [settled, setSettled] = createSignal(false)
  const [error, setError] = createSignal('')
  const { begin } = useLatestRequest()

  const refresh = async (): Promise<ResourceReadResult<T>> => {
    void untrack(revision)
    const request = begin()
    try {
      const payload = await fetchJson<ApiResponse<T>>(path, { signal: request.signal })
      if (!request.isCurrent()) return { status: 'superseded' }
      if (!payload.success || payload.data == null) throw new Error(payload.message || '请求失败')
      const next = payload.data
      setBox((previous) => (isSameResourceValue(previous.value, next) ? previous : { value: next }))
      setSupported(true)
      setError('')
      return { status: 'success', data: next }
    } catch (cause) {
      if (!request.isCurrent()) return { status: 'superseded' }
      if (cause instanceof HttpStatusError && cause.status === 404) {
        setSupported(false)
        setBox({ value: initial })
        setError('')
        return { status: 'success', data: initial }
      }
      const message = cause instanceof Error ? cause.message : String(cause)
      setError(message)
      return { status: 'error', error: message }
    } finally {
      if (request.isCurrent()) setSettled(true)
    }
  }

  return { data, supported, settled, error, refresh }
}

export function useTerminals(revision: Accessor<number> = () => 0) {
  const resource = useOptionalResource<TerminalSummary[]>('/api/terminals', [], revision)
  return {
    terminals: resource.data,
    terminalsSupported: resource.supported,
    terminalsLoaded: resource.settled,
    terminalsError: resource.error,
    fetchTerminals: resource.refresh,
  }
}

export function useAccessPorts(revision: Accessor<number> = () => 0) {
  const resource = useOptionalResource<AccessPortStatus[]>('/api/access-ports', [], revision)
  return {
    accessPorts: resource.data,
    accessPortsSupported: resource.supported,
    accessPortsLoaded: resource.settled,
    accessPortsError: resource.error,
    fetchAccessPorts: resource.refresh,
  }
}
