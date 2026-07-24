import { useState, useCallback, useEffect, useRef, useMemo } from 'react'
import { API_HEADERS } from '../utils.js'
import { useWebSocket } from './useWebSocket.js'

function fetchWithSignal(url, signal) {
  return signal ? fetch(url, { signal }) : fetch(url)
}

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
  const requestIdRef = useRef(0)
  const activeRequestsRef = useRef([])

  const apiCall = useCallback(async (endpoint, options = {}, action = '') => {
    const requestId = ++requestIdRef.current
    activeRequestsRef.current = [...activeRequestsRef.current, { requestId, action }]
    setLoadingAction(action)
    try {
      const response = await fetch(`/api/${endpoint}`, { headers: API_HEADERS, ...options })
      const payload = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } finally {
      activeRequestsRef.current = activeRequestsRef.current.filter(
        (request) => request.requestId !== requestId
      )
      setLoadingAction(activeRequestsRef.current.at(-1)?.action || '')
    }
  }, [setLoadingAction])

  return { apiCall, loadingAction }
}

export function useStatus() {
  const [status, setStatus] = useState({
    running: false,
    pid: null,
    uptime_secs: null,
    initializing: false,
    route_mode: 'rule'
  })
  const requestIdRef = useRef(0)
  const [error, setError] = useState('')
  const [hasLoaded, setHasLoaded] = useState(false)

  const fetchStatus = useCallback(async (signal) => {
    const requestId = ++requestIdRef.current
    try {
      const response = await fetchWithSignal('/api/status', signal)
      if (!response.ok) throw new Error(`状态获取失败 (${response.status})`)
      const payload = await response.json()
      if (!payload.success || !payload.data) {
        throw new Error(payload.message || '状态数据无效')
      }
      if (requestId === requestIdRef.current) {
        setStatus(payload.data)
        setError('')
        setHasLoaded(true)
      }
      return payload.data
    } catch (fetchError) {
      if (requestId === requestIdRef.current) {
        setError(fetchError.message || '状态获取失败')
      }
      return null
    }
  }, [])

  return { status, setStatus, fetchStatus, statusError: error, statusLoaded: hasLoaded }
}

export function useSubs() {
  const [subs, setSubs] = useState([])
  const requestIdRef = useRef(0)
  const [error, setError] = useState('')
  const [hasLoaded, setHasLoaded] = useState(false)

  const fetchSubs = useCallback(async (signal) => {
    const requestId = ++requestIdRef.current
    try {
      const response = await fetchWithSignal('/api/subs', signal)
      if (!response.ok) throw new Error(`订阅获取失败 (${response.status})`)
      const payload = await response.json()
      if (!payload.success || !Array.isArray(payload.data)) {
        throw new Error(payload.message || '订阅数据无效')
      }
      if (requestId === requestIdRef.current) {
        setSubs(payload.data)
        setError('')
        setHasLoaded(true)
      }
      return payload.data
    } catch (fetchError) {
      if (requestId === requestIdRef.current) {
        setError(fetchError.message || '订阅获取失败')
      }
      return null
    }
  }, [])

  return { subs, setSubs, fetchSubs, subsError: error, subsLoaded: hasLoaded }
}

export function useNodes() {
  const [nodes, setNodes] = useState([])
  const requestIdRef = useRef(0)
  const [error, setError] = useState('')
  const [hasLoaded, setHasLoaded] = useState(false)

  const fetchNodes = useCallback(async (signal) => {
    const requestId = ++requestIdRef.current
    try {
      const response = await fetchWithSignal('/api/nodes', signal)
      if (!response.ok) throw new Error(`节点获取失败 (${response.status})`)
      const payload = await response.json()
      if (!payload.success || !Array.isArray(payload.data)) {
        throw new Error(payload.message || '节点数据无效')
      }
      if (requestId === requestIdRef.current) {
        setNodes(payload.data)
        setError('')
        setHasLoaded(true)
      }
      return payload.data
    } catch (fetchError) {
      if (requestId === requestIdRef.current) {
        setError(fetchError.message || '节点获取失败')
      }
      return null
    }
  }, [])

  return { nodes, setNodes, fetchNodes, nodesError: error, nodesLoaded: hasLoaded }
}

export function useProxies(status) {
  const [proxies, setProxies] = useState({})
  const requestIdRef = useRef(0)
  const [error, setError] = useState('')

  const clashApiBase = useMemo(() => '/api/clash', [])

  const fetchProxies = useCallback(async (signal) => {
    const requestId = ++requestIdRef.current
    try {
      const response = await fetchWithSignal(`${clashApiBase}/proxies`, signal)
      if (!response.ok) throw new Error(`代理组获取失败 (${response.status})`)
      const payload = await response.json()
      if (requestId === requestIdRef.current) {
        setProxies(payload.proxies || {})
        setError('')
      }
      return payload.proxies || {}
    } catch (fetchError) {
      if (requestId === requestIdRef.current) {
        setError(fetchError.message || '代理组获取失败')
      }
      return null
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
      requestIdRef.current += 1
      setProxies({})
      setError('')
    }
  }, [status.running])

  return {
    proxies,
    setProxies,
    fetchProxies,
    proxiesError: error,
    selectorGroups,
    primaryGroupName,
    primaryGroup,
  }
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
  const requestIdRef = useRef(0)

  const fetchConnections = useCallback(async (signal) => {
    if (!status.running) {
      setConnectionsInfo({ uploadTotal: 0, downloadTotal: 0, connections: [] })
      setConnectionsError('')
      return null
    }

    const requestId = ++requestIdRef.current
    setConnectionsLoading(true)
    try {
      const response = await fetchWithSignal(`${clashApiBase}/connections`, signal)
      if (!response.ok) {
        const details = (await response.text()).trim()
        throw new Error(details || `连接统计获取失败 (${response.status})`)
      }
      const payload = await response.json()
      if (requestId !== requestIdRef.current) return payload
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
      if (requestId === requestIdRef.current) {
        setConnectionsError(error.message || '连接统计获取失败')
      }
      return null
    } finally {
      if (requestId === requestIdRef.current) {
        setConnectionsLoading(false)
      }
    }
  }, [clashApiBase, status.running])

  useEffect(() => {
    if (!status.running) {
      requestIdRef.current += 1
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
  const requestIdRef = useRef(0)

  const fetchVersion = useCallback(async () => {
    const requestId = ++requestIdRef.current
    try {
      const response = await fetch('/api/version')
      if (!response.ok) throw new Error(`版本获取失败 (${response.status})`)
      const payload = await response.json()
      if (requestId === requestIdRef.current && payload.success && payload.data) {
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
    const pendingNames = [...new Set(nodeNames)]
    let nextIndex = 0

    const worker = async () => {
      while (nextIndex < pendingNames.length) {
        const nodeName = pendingNames[nextIndex]
        nextIndex += 1
        await testDelay(clashApiBase, nodeName)
      }
    }

    try {
      const workerCount = Math.min(8, pendingNames.length)
      await Promise.all(Array.from({ length: workerCount }, worker))
    } finally {
      setTestingGroup('')
    }
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
  const connectivityAbortRef = useRef(null)

  const testSingleSite = useCallback(async (site) => {
    connectivityAbortRef.current?.abort()
    const controller = new AbortController()
    connectivityAbortRef.current = controller
    setCurrentTestingSite(site.name)
    try {
      const response = await fetch('/api/connectivity', {
        method: 'POST',
        headers: API_HEADERS,
        body: JSON.stringify({ url: site.url }),
        signal: controller.signal,
      })
      const payload = await response.json()
      setConnectivityResults((prev) => ({ ...prev, [site.name]: payload.success ? payload.data : { success: false } }))
    } catch (error) {
      if (error.name === 'AbortError') return
      setConnectivityResults((prev) => ({ ...prev, [site.name]: { success: false } }))
    } finally {
      if (connectivityAbortRef.current === controller) {
        connectivityAbortRef.current = null
        setCurrentTestingSite(null)
      }
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
    connectivityAbortRef.current?.abort()
    connectivityAbortRef.current = null
    setTestingConnectivity(false)
    setCurrentTestingSite(null)
  }, [])

  const clearConnectivity = useCallback(() => {
    stopConnectivityRef.current = true
    connectivityAbortRef.current?.abort()
    connectivityAbortRef.current = null
    setTestingConnectivity(false)
    setCurrentTestingSite(null)
    setConnectivityResults({})
  }, [])

  useEffect(() => {
    return () => connectivityAbortRef.current?.abort()
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
