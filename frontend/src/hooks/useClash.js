import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useWebSocket } from './useWebSocket.js'

export function useProxies(status) {
  const [proxies, setProxies] = useState({})
  const clashApiBase = useMemo(() => '/api/clash', [])

  const fetchProxies = useCallback(async () => {
    try {
      const response = await fetch(`${clashApiBase}/proxies`)
      const payload = await response.json()
      setProxies(payload.proxies || {})
    } catch {
      setProxies({})
    }
  }, [clashApiBase])

  const selectorGroups = useMemo(() => {
    const groups = {}
    Object.entries(proxies || {}).forEach(([name, proxy]) => {
      if (proxy?.type === 'Selector') groups[name] = proxy
    })
    return groups
  }, [proxies])

  const primaryGroupName = selectorGroups.proxy ? 'proxy' : Object.keys(selectorGroups)[0]
  const primaryGroup = primaryGroupName ? selectorGroups[primaryGroupName] : null

  useEffect(() => {
    if (!status.running) setProxies({})
  }, [status.running])

  return { proxies, fetchProxies, selectorGroups, primaryGroupName, primaryGroup }
}

export function useTraffic(status) {
  const [traffic, setTraffic] = useState({})

  const trafficUrl = useMemo(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    return `${protocol}//${window.location.host}/api/clash/traffic`
  }, [])

  const handleMessage = useCallback((data) => {
    if (data && typeof data.up === 'number' && typeof data.down === 'number') {
      setTraffic({ up: data.up, down: data.down })
    }
  }, [])

  const { close: closeSockets } = useWebSocket(trafficUrl, handleMessage, status.running)

  useEffect(() => {
    if (!status.running) setTraffic({})
  }, [status.running])

  return { traffic, closeSockets }
}

export function useConnections(status, clashApiBase) {
  const [connectionsInfo, setConnectionsInfo] = useState({ uploadTotal: 0, downloadTotal: 0, connections: [] })
  const [connectionsLoading, setConnectionsLoading] = useState(false)
  const [connectionsError, setConnectionsError] = useState('')
  const lastConnectionsRef = useRef({ at: 0, connections: new Map() })

  const fetchConnections = useCallback(async () => {
    if (!status.running) {
      setConnectionsInfo({ uploadTotal: 0, downloadTotal: 0, connections: [] })
      setConnectionsError('')
      return null
    }

    setConnectionsLoading(true)
    try {
      const response = await fetch(`${clashApiBase}/connections`)
      if (!response.ok) {
        const details = (await response.text()).trim()
        throw new Error(details || `连接统计获取失败 (${response.status})`)
      }
      const payload = await response.json()
      const connections = Array.isArray(payload.connections) ? payload.connections : []
      const now = Date.now()
      const previous = lastConnectionsRef.current
      const elapsedSecs = previous.at ? Math.max((now - previous.at) / 1000, 1) : 0
      const currentMap = new Map()
      const enrichedConnections = connections.map((connection) => {
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
      setConnectionsError(error.message || '连接统计获取失败')
      return null
    } finally {
      setConnectionsLoading(false)
    }
  }, [clashApiBase, status.running])

  useEffect(() => {
    if (!status.running) {
      setConnectionsInfo({ uploadTotal: 0, downloadTotal: 0, connections: [] })
      setConnectionsError('')
      setConnectionsLoading(false)
      lastConnectionsRef.current = { at: 0, connections: new Map() }
    }
  }, [status.running])

  const closeConnection = useCallback(async (id) => {
    const response = await fetch(`${clashApiBase}/connections/${encodeURIComponent(id)}`, { method: 'DELETE' })
    if (!response.ok) {
      const details = (await response.text()).trim()
      throw new Error(details || `关闭连接失败 (${response.status})`)
    }
    await fetchConnections()
  }, [clashApiBase, fetchConnections])

  return {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
    closeConnection,
  }
}

export function useDelays() {
  const [delays, setDelays] = useState({})
  const [testingNodes, setTestingNodes] = useState({})
  const [testingGroup, setTestingGroup] = useState('')

  const testDelay = useCallback(async (clashApiBase, nodeName) => {
    setTestingNodes((prev) => ({ ...prev, [nodeName]: true }))
    try {
      const response = await fetch(`${clashApiBase}/proxies/${encodeURIComponent(nodeName)}/delay?timeout=3000&url=http://www.gstatic.com/generate_204`)
      if (!response.ok) {
        setDelays((prev) => ({ ...prev, [nodeName]: -1 }))
        return
      }
      const payload = await response.json()
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

  const testGroupDelays = useCallback(async (clashApiBase, groupName, nodeNames) => {
    setTestingGroup(groupName)
    await Promise.all([...new Set(nodeNames)].map((name) => testDelay(clashApiBase, name)))
    setTestingGroup('')
  }, [testDelay])

  const clearDelays = useCallback(() => setDelays({}), [])

  return { delays, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays }
}
