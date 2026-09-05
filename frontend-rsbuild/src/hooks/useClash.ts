import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { CLASH_API_BASE, DELAY_TEST_URL } from '../utils'
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

export function isClashProxyGroup(type: string | undefined): boolean {
  return type === 'Selector' || type === 'URLTest'
}

export function useProxies(status: Pick<StatusData, 'ready'>) {
  const [proxies, setProxies] = useState<ClashProxies>({})
  const clashApiBase = CLASH_API_BASE
  const { begin, cancel } = useLatestRequest()

  const fetchProxies = useCallback(async () => {
    const request = begin()
    try {
      const payload = await fetchJson<ClashProxiesPayload>(`${clashApiBase}/proxies`, { signal: request.signal })
      if (request.isCurrent()) setProxies(payload.proxies || {})
    } catch {
      // Keep the last snapshot through transient failures.
    }
  }, [clashApiBase, begin])

  const selectorGroups = useMemo(() => {
    const groups: Record<string, ClashProxy> = {}
    Object.entries(proxies || {}).forEach(([name, proxy]) => {
      if (isClashProxyGroup(proxy?.type)) groups[name] = proxy
    })
    return groups
  }, [proxies])

  const primaryGroupName = selectorGroups.proxy ? 'proxy' : Object.keys(selectorGroups)[0]
  const primaryGroup = primaryGroupName ? selectorGroups[primaryGroupName] : null

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

export function useConnections(status: Pick<StatusData, 'ready'>, clashApiBase: string) {
  const [connectionsInfo, setConnectionsInfo] = useState<ConnectionsInfo>(EMPTY_CONNECTIONS)
  const [connectionsLoading, setConnectionsLoading] = useState(false)
  const [connectionsError, setConnectionsError] = useState('')
  const lastConnectionsRef = useRef<{ at: number; connections: Map<string, ClashConnection> }>({ at: 0, connections: new Map() })
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
      const connections = Array.isArray(payload.connections) ? payload.connections : []
      const now = Date.now()
      const previous = lastConnectionsRef.current
      const elapsedSecs = previous.at ? Math.max((now - previous.at) / 1000, 1) : 0
      const currentMap = new Map<string, ClashConnection>()
      const enrichedConnections: EnrichedConnection[] = connections.map((connection) => {
        currentMap.set(connection.id, connection)
        const last = previous.connections.get(connection.id)
        const uploadSpeed = last && elapsedSecs
          ? Math.max(0, Number(connection.upload || 0) - Number(last.upload || 0)) / elapsedSecs
          : 0
        const downloadSpeed = last && elapsedSecs
          ? Math.max(0, Number(connection.download || 0) - Number(last.download || 0)) / elapsedSecs
          : 0
        return { ...connection, uploadSpeed, downloadSpeed }
      })
      lastConnectionsRef.current = { at: now, connections: currentMap }
      setConnectionsInfo({
        ...payload,
        uploadTotal: Number(payload.uploadTotal || 0),
        downloadTotal: Number(payload.downloadTotal || 0),
        connections: enrichedConnections,
      })
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
    const current = () => requests.current.get(nodeName) === controller
    setTestingNodes(prev => ({ ...prev, [nodeName]: true }))
    let delay = -1
    try {
      const payload = await fetchJson<ClashDelay>(
        `${clashApiBase}/proxies/${encodeURIComponent(nodeName)}/delay?timeout=3000&url=${DELAY_TEST_URL}`,
        { signal: controller.signal },
      )
      delay = payload.delay > 0 ? payload.delay : -1
    } catch {
      // A current failed measurement is displayed as unavailable.
    } finally {
      if (current()) {
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
    const workers = Array.from({ length: Math.min(6, queue.length) }, async () => {
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
