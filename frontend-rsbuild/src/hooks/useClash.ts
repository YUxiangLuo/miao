import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { CLASH_API_BASE, DELAY_TEST_TIMEOUT_MS, DELAY_TEST_URL } from '../utils'
import { useWebSocket } from './useWebSocket'
import { fetchJson } from './request'
import { useLatestRequest } from './useLatestRequest'
import type { StatusData } from '../types/api'
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

export function useProxies(status: Pick<StatusData, 'ready'>) {
  const [proxies, setProxies] = useState<ClashProxies>({})
  const clashApiBase = CLASH_API_BASE
  const { begin, cancel } = useLatestRequest()
  const readyRef = useRef(status.ready)
  readyRef.current = status.ready

  const fetchProxies = useCallback(async () => {
    if (!readyRef.current) return
    const request = begin()
    try {
      const payload = await fetchJson<ClashProxiesPayload>(`${clashApiBase}/proxies`, { signal: request.signal })
      if (readyRef.current && request.isCurrent()) setProxies(payload.proxies || {})
    } catch {
      // Keep the last snapshot through transient failures.
    }
  }, [clashApiBase, begin])

  const { primaryGroupName, primaryGroup } = useMemo(
    () => selectPrimaryProxyGroup(proxies),
    [proxies],
  )

  useEffect(() => {
    if (!status.ready) {
      cancel()
      setProxies({})
    }
  }, [status.ready, cancel])

  return { proxies, fetchProxies, primaryGroupName, primaryGroup }
}

export function useTraffic(status: Pick<StatusData, 'ready'>) {
  const [traffic, setTraffic] = useState<Partial<ClashTraffic>>({})

  const trafficUrl = useMemo(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    return `${protocol}//${window.location.host}/api/clash/traffic`
  }, [])

  const handleMessage = useCallback((data: ClashTraffic) => {
    if (data && typeof data.up === 'number' && typeof data.down === 'number') {
      setTraffic({ up: data.up, down: data.down })
    }
  }, [])

  const { close: closeSockets } = useWebSocket<ClashTraffic>(trafficUrl, handleMessage, Boolean(status.ready))

  useEffect(() => {
    if (!status.ready) setTraffic({})
  }, [status.ready])

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

export function useConnections(status: Pick<StatusData, 'ready'>, clashApiBase: string) {
  const [connectionsInfo, setConnectionsInfo] = useState<ConnectionsInfo>(EMPTY_CONNECTIONS)
  const [connectionsLoading, setConnectionsLoading] = useState(false)
  const [connectionsError, setConnectionsError] = useState('')
  const lastConnectionsRef = useRef<ConnectionSnapshot>({ at: 0, connections: new Map() })
  const { begin, cancel } = useLatestRequest()

  const fetchConnections = useCallback(async (): Promise<ClashConnectionsPayload | null> => {
    const request = begin()
    if (!status.ready) {
      setConnectionsInfo(EMPTY_CONNECTIONS)
      setConnectionsError('')
      return null
    }

    setConnectionsLoading(true)
    try {
      const payload = await fetchJson<ClashConnectionsPayload>(`${clashApiBase}/connections`, { signal: request.signal })
      if (!request.isCurrent()) return null
      const { snapshot, info } = enrichConnectionsSnapshot(payload, lastConnectionsRef.current, Date.now())
      lastConnectionsRef.current = snapshot
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
  }, [clashApiBase, status.ready, begin])

  useEffect(() => {
    if (!status.ready) {
      cancel()
      setConnectionsInfo(EMPTY_CONNECTIONS)
      setConnectionsError('')
      setConnectionsLoading(false)
      lastConnectionsRef.current = { at: 0, connections: new Map() }
    }
  }, [status.ready, cancel])

  return {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
  }
}

export function useDelays() {
  const [delays, setDelays] = useState<Record<string, number>>({})
  const [delayMeasuredAt, setDelayMeasuredAt] = useState<Record<string, number>>({})
  const [testingNodes, setTestingNodes] = useState<Record<string, boolean>>({})
  const [testingGroup, setTestingGroup] = useState('')
  const requests = useRef(new Map<string, AbortController>())
  const generation = useRef(0)
  const groupRunning = useRef(false)

  const cancel = useCallback(() => {
    generation.current++
    requests.current.forEach(controller => controller.abort())
    requests.current.clear()
    groupRunning.current = false
  }, [])
  useEffect(() => cancel, [cancel])

  const testDelay = useCallback(async (clashApiBase: string, nodeName: string) => {
    requests.current.get(nodeName)?.abort()
    const controller = new AbortController()
    requests.current.set(nodeName, controller)
    const isCurrentProbe = () => requests.current.get(nodeName) === controller
    setTestingNodes(prev => ({ ...prev, [nodeName]: true }))
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
        setDelays(prev => ({ ...prev, [nodeName]: delay }))
        setDelayMeasuredAt(prev => ({ ...prev, [nodeName]: Date.now() }))
        requests.current.delete(nodeName)
        setTestingNodes(prev => {
          const next = { ...prev }
          delete next[nodeName]
          return next
        })
      }
    }
  }, [])

  const testGroupDelays = useCallback(async (clashApiBase: string, groupName: string, nodeNames: string[]) => {
    if (groupRunning.current) return
    groupRunning.current = true
    const currentGeneration = generation.current
    setTestingGroup(groupName)
    const queue = [...new Set(nodeNames)]
    let next = 0
    const workers = Array.from({ length: Math.min(DELAY_BATCH_CONCURRENCY, queue.length) }, async () => {
      while (generation.current === currentGeneration && next < queue.length) {
        await testDelay(clashApiBase, queue[next++])
      }
    })
    await Promise.all(workers)
    if (generation.current === currentGeneration) {
      groupRunning.current = false
      setTestingGroup('')
    }
  }, [testDelay])

  const clearDelays = useCallback(() => {
    cancel()
    setDelays({})
    setDelayMeasuredAt({})
    setTestingNodes({})
    setTestingGroup('')
  }, [cancel])

  return { delays, delayMeasuredAt, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays }
}
