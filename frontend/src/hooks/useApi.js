import { useState, useCallback, useEffect, useRef, useMemo } from 'react'
import { POLL_INTERVAL, API_HEADERS } from '../utils.js'
import { useWebSocket } from './useWebSocket.js'

export function useToast() {
  const [toasts, setToasts] = useState([])
  const toastIdRef = useRef(0)

  const showToast = useCallback((message, tone = 'info') => {
    const id = ++toastIdRef.current
    setToasts((prev) => [...prev, { id, message, tone }])
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((item) => item.id !== id))
    }, 3500)
  }, [])

  return { toasts, showToast }
}

export function useApi(loadingState) {
  const { loadingAction, setLoadingAction } = loadingState

  const apiCall = useCallback(async (endpoint, options = {}, action = '') => {
    setLoadingAction(action)
    try {
      const response = await fetch(`/api/${endpoint}`, { headers: API_HEADERS, ...options })
      const payload = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } finally {
      setLoadingAction('')
    }
  }, [setLoadingAction])

  return { apiCall, loadingAction }
}

export function useStatus() {
  const [status, setStatus] = useState({ running: false, pid: null, uptime_secs: null, initializing: false })

  const fetchStatus = useCallback(async () => {
    try {
      const response = await fetch('/api/status')
      const payload = await response.json()
      if (payload.success && payload.data) {
        setStatus(payload.data)
      }
    } catch {
      // ignore
    }
  }, [])

  return { status, setStatus, fetchStatus }
}

export function useSubs() {
  const [subs, setSubs] = useState([])

  const fetchSubs = useCallback(async () => {
    try {
      const response = await fetch('/api/subs')
      const payload = await response.json()
      if (payload.success && payload.data) setSubs(payload.data)
    } catch {
      // ignore
    }
  }, [])

  return { subs, setSubs, fetchSubs }
}

export function useNodes() {
  const [nodes, setNodes] = useState([])

  const fetchNodes = useCallback(async () => {
    try {
      const response = await fetch('/api/nodes')
      const payload = await response.json()
      if (payload.success && payload.data) setNodes(payload.data)
    } catch {
      // ignore
    }
  }, [])

  return { nodes, setNodes, fetchNodes }
}

export function useProxies(status) {
  const [proxies, setProxies] = useState({})
  
  const clashApiBase = useMemo(() => `http://${window.location.hostname}:6262`, [])

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

  // 服务停止时清空 proxies
  useEffect(() => {
    if (!status.running) {
      setProxies({})
    }
  }, [status.running])

  return { proxies, setProxies, fetchProxies, selectorGroups, primaryGroupName, primaryGroup }
}

export function useTraffic(status) {
  const [traffic, setTraffic] = useState({})

  const trafficUrl = useMemo(() => `ws://${window.location.hostname}:6262/traffic`, [])

  const handleMessage = useCallback((data) => {
    if (data && typeof data.up === 'number' && typeof data.down === 'number') {
      setTraffic({ up: data.up, down: data.down })
    }
  }, [])

  const { close: closeSockets } = useWebSocket(trafficUrl, handleMessage, status.running)

  // 服务停止时清空流量数据
  useEffect(() => {
    if (!status.running) {
      setTraffic({})
    }
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

  const closeAllConnections = useCallback(async () => {
    const response = await fetch(`${clashApiBase}/connections`, { method: 'DELETE' })
    if (!response.ok) {
      const details = (await response.text()).trim()
      throw new Error(details || `关闭全部连接失败 (${response.status})`)
    }
    await fetchConnections()
  }, [clashApiBase, fetchConnections])

  return {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
    closeConnection,
    closeAllConnections,
  }
}

export function useVersion() {
  const [versionInfo, setVersionInfo] = useState({ current: '', latest: null, has_update: false })

  const fetchVersion = useCallback(async () => {
    try {
      const response = await fetch('/api/version')
      const payload = await response.json()
      if (payload.success && payload.data) {
        setVersionInfo(payload.data)
        return payload.data
      }
    } catch {
      // ignore
    }
    return null
  }, [])

  return { versionInfo, setVersionInfo, fetchVersion }
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

  const clearDelays = useCallback(() => {
    setDelays({})
  }, [])

  return { delays, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays }
}

export function useConnectivity() {
  const [connectivityResults, setConnectivityResults] = useState({})
  const [testingConnectivity, setTestingConnectivity] = useState(false)
  const [currentTestingSite, setCurrentTestingSite] = useState(null)
  const stopConnectivityRef = useRef(false)

  const testSingleSite = useCallback(async (site) => {
    setCurrentTestingSite(site.name)
    try {
      const response = await fetch('/api/connectivity', {
        method: 'POST',
        headers: API_HEADERS,
        body: JSON.stringify({ url: site.url }),
      })
      const payload = await response.json()
      setConnectivityResults((prev) => ({ ...prev, [site.name]: payload.success ? payload.data : { success: false } }))
    } catch {
      setConnectivityResults((prev) => ({ ...prev, [site.name]: { success: false } }))
    } finally {
      setCurrentTestingSite(null)
    }
  }, [])

  const testAllConnectivity = useCallback(async (sites) => {
    setTestingConnectivity(true)
    stopConnectivityRef.current = false
    setConnectivityResults({})
    for (const site of sites) {
      if (stopConnectivityRef.current) break
      await testSingleSite(site)
    }
    setTestingConnectivity(false)
    stopConnectivityRef.current = false
  }, [testSingleSite])

  const stopConnectivity = useCallback(() => {
    stopConnectivityRef.current = true
    setTestingConnectivity(false)
    setCurrentTestingSite(null)
  }, [])

  const clearConnectivity = useCallback(() => {
    setConnectivityResults({})
  }, [])

  return { 
    connectivityResults, 
    testingConnectivity, 
    currentTestingSite, 
    testSingleSite, 
    testAllConnectivity, 
    stopConnectivity,
    clearConnectivity 
  }
}
