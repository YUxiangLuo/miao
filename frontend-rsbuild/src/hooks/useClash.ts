import { createEffect, createMemo, createSignal, onCleanup, untrack, type Accessor } from 'solid-js'
import { CLASH_API_BASE, DELAY_TEST_TIMEOUT_MS, DELAY_TEST_URL } from '../utils'
import { useWebSocket } from './useWebSocket'
import { fetchJson } from './request'
import { useLatestRequest } from './useLatestRequest'
import type {
  ClashConnection,
  ClashConnectionsPayload,
  ClashDelay,
  ClashProxies,
  ClashProxiesPayload,
  ClashProxy,
  ClashTraffic,
  ConnectionsInfo,
  EnrichedConnection,
} from '../types/clash'

export const DELAY_BATCH_CONCURRENCY = 6

export function isClashProxyGroup(type: string | undefined): boolean {
  return type === 'Selector' || type === 'URLTest'
}

export function selectPrimaryProxyGroup(proxies: ClashProxies | null | undefined) {
  const selectorGroups: Record<string, ClashProxy> = {}
  for (const [name, proxy] of Object.entries(proxies || {})) {
    if (isClashProxyGroup(proxy?.type)) selectorGroups[name] = proxy
  }
  const primaryGroupName = selectorGroups.proxy ? 'proxy' : Object.keys(selectorGroups)[0]
  const primaryGroup = primaryGroupName ? selectorGroups[primaryGroupName] : null
  return { selectorGroups, primaryGroupName, primaryGroup }
}

/** 节点名 → 协议类型（Clash API 的 type，如 Hysteria2/AnyTLS/VLESS）；分组项不入图 */
export function collectNodeProtocols(proxies: ClashProxies): Record<string, string> {
  const protocols: Record<string, string> = {}
  for (const [name, proxy] of Object.entries(proxies || {})) {
    if (proxy?.type && !isClashProxyGroup(proxy.type)) {
      protocols[name] = proxy.type
    }
  }
  return protocols
}

/**
 * 规则「指定节点」下拉候选：手动节点（服务停止时也在）∪ 运行时全部 outbound。
 * 与后端 known_rule_targets 同口径（排除内置 proxy/direct 与分组项），不随 fastest_* 地区过滤收缩。
 */
export function collectRuleTargetNodes(
  manualNodes: ReadonlyArray<{ tag: string }>,
  proxies: ClashProxies,
): string[] {
  const names = new Set<string>(manualNodes.map((node) => node.tag))
  for (const [name, proxy] of Object.entries(proxies || {})) {
    if (name !== 'proxy' && name !== 'direct' && !isClashProxyGroup(proxy?.type)) {
      names.add(name)
    }
  }
  return [...names]
}

export function latestClashHistorySample(
  history: ClashProxy['history'],
): { delay: number; measuredAt: number } | null {
  if (!history?.length) return null
  const latest = history.reduce((newest, item) => (
    Date.parse(item.time) > Date.parse(newest.time) ? item : newest
  ))
  if (!latest.delay) return null
  return { delay: latest.delay, measuredAt: Date.parse(latest.time) }
}

/**
 * fastest (URLTest) 模式的延迟展示回落到 /proxies 自带的周期测速 history；
 * 手动点测结果与 history 按测量时间合并，更新鲜的一方优先展示。
 */
export function mergeDisplayMeasurements({
  proxies,
  isUrlTestGroup,
  delays,
  delayMeasuredAt,
}: {
  proxies: ClashProxies
  isUrlTestGroup: boolean
  delays: Record<string, number>
  delayMeasuredAt: Record<string, number>
}): { delays: Record<string, number>; measuredAt: Record<string, number> } {
  const mergedDelays = { ...delays }
  const mergedMeasuredAt = { ...delayMeasuredAt }
  if (!isUrlTestGroup) {
    return { delays: mergedDelays, measuredAt: mergedMeasuredAt }
  }

  for (const [name, proxy] of Object.entries(proxies || {})) {
    if (isClashProxyGroup(proxy?.type)) continue
    const sample = latestClashHistorySample(proxy?.history)
    if (!sample) continue
    const manualTimestamp = delayMeasuredAt[name] ?? 0
    if (!(name in mergedDelays) || sample.measuredAt > manualTimestamp) {
      mergedDelays[name] = sample.delay
      mergedMeasuredAt[name] = sample.measuredAt
    }
  }

  return { delays: mergedDelays, measuredAt: mergedMeasuredAt }
}

export function useProxies(ready: Accessor<boolean>) {
  const [proxies, setProxies] = createSignal<ClashProxies>({})
  const clashApiBase = CLASH_API_BASE
  const { begin, cancel } = useLatestRequest()

  const fetchProxies = async () => {
    if (!untrack(ready)) return
    const request = begin()
    try {
      const payload = await fetchJson<ClashProxiesPayload>(`${clashApiBase}/proxies`, { signal: request.signal })
      if (untrack(ready) && request.isCurrent()) setProxies(payload.proxies || {})
    } catch {
      // Keep the last snapshot through transient failures.
    }
  }

  const selection = createMemo(() => selectPrimaryProxyGroup(proxies()))
  const primaryGroupName = createMemo(() => selection().primaryGroupName || '')
  const primaryGroup = createMemo(() => selection().primaryGroup)

  createEffect(
    () => ready(),
    (isReady) => {
      if (!isReady) {
        cancel()
        setProxies({})
      }
    },
  )

  return { proxies, fetchProxies, primaryGroupName, primaryGroup }
}

export function useTraffic(ready: Accessor<boolean>) {
  const [traffic, setTraffic] = createSignal<Partial<ClashTraffic>>({})
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  const trafficUrl = `${protocol}//${window.location.host}/api/clash/traffic`

  const handleMessage = (data: ClashTraffic) => {
    if (data && typeof data.up === 'number' && typeof data.down === 'number') {
      setTraffic({ up: data.up, down: data.down })
    }
  }

  const { close: closeSockets } = useWebSocket<ClashTraffic>(trafficUrl, handleMessage, ready)

  createEffect(
    () => ready(),
    (isReady) => {
      if (!isReady) setTraffic({})
    },
  )

  return { traffic, closeSockets }
}

const EMPTY_CONNECTIONS: ConnectionsInfo = { uploadTotal: 0, downloadTotal: 0, connections: [] }

export interface ConnectionSnapshot {
  at: number
  connections: Map<string, ClashConnection>
}

export function enrichConnectionsSnapshot(
  payload: ClashConnectionsPayload,
  previous: ConnectionSnapshot,
  now: number,
): { snapshot: ConnectionSnapshot; info: ConnectionsInfo } {
  const rawConnections = Array.isArray(payload.connections) ? payload.connections : []
  const elapsedSecs = previous.at ? Math.max((now - previous.at) / 1000, 1) : 0
  const currentMap = new Map<string, ClashConnection>()

  const enrichedConnections: EnrichedConnection[] = rawConnections.map((connection) => {
    currentMap.set(connection.id, connection)
    const last = previous.connections.get(connection.id)
    if (!last || !elapsedSecs) {
      return { ...connection, uploadSpeed: 0, downloadSpeed: 0 }
    }
    const uploadDelta = Math.max(0, Number(connection.upload || 0) - Number(last.upload || 0))
    const downloadDelta = Math.max(0, Number(connection.download || 0) - Number(last.download || 0))
    return {
      ...connection,
      uploadSpeed: uploadDelta / elapsedSecs,
      downloadSpeed: downloadDelta / elapsedSecs,
    }
  })

  return {
    snapshot: { at: now, connections: currentMap },
    info: {
      ...payload,
      uploadTotal: Number(payload.uploadTotal || 0),
      downloadTotal: Number(payload.downloadTotal || 0),
      connections: enrichedConnections,
    },
  }
}

export function normalizeClashDelay(delay: number | undefined): number {
  return delay !== undefined && delay > 0 ? delay : -1
}

export function useConnections(ready: Accessor<boolean>, clashApiBase: string) {
  const [connectionsInfo, setConnectionsInfo] = createSignal<ConnectionsInfo>(EMPTY_CONNECTIONS)
  const [connectionsLoading, setConnectionsLoading] = createSignal(false)
  const [connectionsError, setConnectionsError] = createSignal('')
  let lastConnections: ConnectionSnapshot = { at: 0, connections: new Map() }
  const { begin, cancel } = useLatestRequest()

  const fetchConnections = async (): Promise<ClashConnectionsPayload | null> => {
    const request = begin()
    if (!untrack(ready)) {
      setConnectionsInfo(EMPTY_CONNECTIONS)
      setConnectionsError('')
      return null
    }

    setConnectionsLoading(true)
    try {
      const payload = await fetchJson<ClashConnectionsPayload>(`${clashApiBase}/connections`, { signal: request.signal })
      if (!request.isCurrent()) return null
      const { snapshot, info } = enrichConnectionsSnapshot(payload, lastConnections, Date.now())
      lastConnections = snapshot
      setConnectionsInfo(info)
      setConnectionsError('')
      return payload
    } catch (error) {
      if (request.isCurrent()) {
        setConnectionsError(error instanceof Error ? error.message : '链接统计获取失败')
      }
      return null
    } finally {
      if (request.isCurrent()) setConnectionsLoading(false)
    }
  }

  createEffect(
    () => ready(),
    (isReady) => {
      if (!isReady) {
        cancel()
        setConnectionsInfo(EMPTY_CONNECTIONS)
        setConnectionsError('')
        setConnectionsLoading(false)
        lastConnections = { at: 0, connections: new Map() }
      }
    },
  )

  return {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
  }
}

export function useDelays() {
  const [delays, setDelays] = createSignal<Record<string, number>>({})
  const [delayMeasuredAt, setDelayMeasuredAt] = createSignal<Record<string, number>>({})
  const [testingNodes, setTestingNodes] = createSignal<Record<string, boolean>>({})
  const [testingGroup, setTestingGroup] = createSignal('')
  const requests = new Map<string, AbortController>()
  let generation = 0
  let groupRunning = false

  const cancel = () => {
    generation++
    requests.forEach((controller) => controller.abort())
    requests.clear()
    groupRunning = false
  }
  onCleanup(cancel)

  const testDelay = async (clashApiBase: string, nodeName: string) => {
    requests.get(nodeName)?.abort()
    const controller = new AbortController()
    requests.set(nodeName, controller)
    const isCurrentProbe = () => requests.get(nodeName) === controller
    setTestingNodes((prev) => ({ ...prev, [nodeName]: true }))
    let delay = -1
    try {
      const payload = await fetchJson<ClashDelay>(
        `${clashApiBase}/proxies/${encodeURIComponent(nodeName)}/delay?timeout=${DELAY_TEST_TIMEOUT_MS}&url=${DELAY_TEST_URL}`,
        { signal: controller.signal },
      )
      delay = normalizeClashDelay(payload.delay)
    } catch {
      // A current failed measurement is displayed as unavailable.
    } finally {
      if (isCurrentProbe()) {
        setDelays((prev) => ({ ...prev, [nodeName]: delay }))
        setDelayMeasuredAt((prev) => ({ ...prev, [nodeName]: Date.now() }))
        requests.delete(nodeName)
        setTestingNodes((prev) => {
          const next = { ...prev }
          delete next[nodeName]
          return next
        })
      }
    }
  }

  const testGroupDelays = async (clashApiBase: string, groupName: string, nodeNames: string[]) => {
    if (groupRunning) return
    groupRunning = true
    const currentGeneration = generation
    setTestingGroup(groupName)
    const queue = [...new Set(nodeNames)]
    let next = 0
    const workers = Array.from({ length: Math.min(DELAY_BATCH_CONCURRENCY, queue.length) }, async () => {
      while (generation === currentGeneration && next < queue.length) {
        await testDelay(clashApiBase, queue[next++])
      }
    })
    await Promise.all(workers)
    if (generation === currentGeneration) {
      groupRunning = false
      setTestingGroup('')
    }
  }

  const clearDelays = () => {
    cancel()
    setDelays({})
    setDelayMeasuredAt({})
    setTestingNodes({})
    setTestingGroup('')
  }

  return { delays, delayMeasuredAt, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays }
}
