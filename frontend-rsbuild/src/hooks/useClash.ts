import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { CLASH_API_BASE, DELAY_TEST_URL } from '../utils'
import { useWebSocket } from './useWebSocket'
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

  const fetchProxies = useCallback(async () => {
    try {
      const response = await fetch(`${clashApiBase}/proxies`)
      const payload: ClashProxiesPayload = await response.json()
      setProxies(payload.proxies || {})
    } catch {
      setProxies({})
    }
  }, [clashApiBase])

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
    if (!status.ready) setProxies({})
  }, [status.ready])

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
  const requestGenerationRef = useRef(0)

  const fetchConnections = useCallback(async (): Promise<ClashConnectionsPayload | null> => {
    const generation = ++requestGenerationRef.current
    if (!status.ready) {
      setConnectionsInfo(EMPTY_CONNECTIONS)
      setConnectionsError('')
      return null
    }

    setConnectionsLoading(true)
    try {
      const response = await fetch(`${clashApiBase}/connections`)
      if (!response.ok) {
        const details = (await response.text()).trim()
        throw new Error(details || `链接统计获取失败 (${response.status})`)
      }
      const payload: ClashConnectionsPayload = await response.json()
      if (generation !== requestGenerationRef.current) return payload
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
      if (generation === requestGenerationRef.current) {
        setConnectionsError(error instanceof Error ? error.message : '链接统计获取失败')
      }
      return null
    } finally {
      if (generation === requestGenerationRef.current) setConnectionsLoading(false)
    }
  }, [clashApiBase, status.ready])

  useEffect(() => {
    if (!status.ready) {
      requestGenerationRef.current += 1
      setConnectionsInfo(EMPTY_CONNECTIONS)
      setConnectionsError('')
      setConnectionsLoading(false)
      lastConnectionsRef.current = { at: 0, connections: new Map() }
    }
  }, [status.ready])

  return {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
  }
}

export function useDelays() {
  const [delays, setDelays] = useState<Record<string, number>>({})
  const [testingNodes, setTestingNodes] = useState<Record<string, boolean>>({})
  const [testingGroup, setTestingGroup] = useState('')

  const testDelay = useCallback(async (clashApiBase: string, nodeName: string) => {
    setTestingNodes((prev) => ({ ...prev, [nodeName]: true }))
    try {
      const response = await fetch(`${clashApiBase}/proxies/${encodeURIComponent(nodeName)}/delay?timeout=3000&url=${DELAY_TEST_URL}`)
      if (!response.ok) {
        setDelays((prev) => ({ ...prev, [nodeName]: -1 }))
        return
      }
      const payload: ClashDelay = await response.json()
      setDelays((prev) => ({ ...prev, [nodeName]: payload.delay > 0 ? payload.delay : -1 }))
    } catch {
      setDelays((prev) => ({ ...prev, [nodeName]: -1 }))
    } finally {
      setTestingNodes((prev) => {
        const next = { ...prev }
        delete next[nodeName]
        return next
      })
    }
  }, [])

  const testGroupDelays = useCallback(async (clashApiBase: string, groupName: string, nodeNames: string[]) => {
    setTestingGroup(groupName)
    // 限制并发，避免大订阅一次性发出数百个延迟测试请求
    const queue = [...new Set(nodeNames)]
    const workers = Array.from(
      { length: Math.min(6, queue.length) },
      async () => {
        while (queue.length > 0) {
          const name = queue.shift()!
          await testDelay(clashApiBase, name)
        }
      }
    )
    await Promise.all(workers)
    setTestingGroup('')
  }, [testDelay])

  const clearDelays = useCallback(() => setDelays({}), [])

  return { delays, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays }
}
