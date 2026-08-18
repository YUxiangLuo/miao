import { useCallback, useState } from 'react'

export function useStatus() {
  const [status, setStatus] = useState({
    running: false,
    pid: null,
    uptime_secs: null,
    initializing: false,
    route_mode: 'rule',
    node_select: 'manual',
    adblock: false
  })
  // 是否成功拿到过后端响应：区分「服务未运行」与「后端根本没起来」
  const [statusLoaded, setStatusLoaded] = useState(false)
  // 连续失败次数（成功即清零），作为面板断线提示的健康度信号
  const [statusFailures, setStatusFailures] = useState(0)

  const fetchStatus = useCallback(async () => {
    try {
      const response = await fetch('/api/status')
      const payload = await response.json()
      if (payload.success && payload.data) {
        setStatus(payload.data)
        setStatusLoaded(true)
        setStatusFailures(0)
        return
      }
    } catch {
      // 失败计数在下方统一处理
    }
    // Keep the last known state during transient failures.
    setStatusFailures((count) => count + 1)
  }, [])

  return { status, statusLoaded, statusFailures, fetchStatus }
}

export function useSubs() {
  const [subs, setSubs] = useState([])

  const fetchSubs = useCallback(async () => {
    try {
      const response = await fetch('/api/subs')
      const payload = await response.json()
      if (payload.success && payload.data) setSubs(payload.data)
    } catch {
      // Keep the last known state during transient failures.
    }
  }, [])

  return { subs, fetchSubs }
}

export function useNodes() {
  const [nodes, setNodes] = useState([])

  const fetchNodes = useCallback(async () => {
    try {
      const response = await fetch('/api/nodes')
      const payload = await response.json()
      if (payload.success && payload.data) setNodes(payload.data)
    } catch {
      // Keep the last known state during transient failures.
    }
  }, [])

  return { nodes, fetchNodes }
}

export function useRules() {
  const [rules, setRules] = useState([])

  const fetchRules = useCallback(async () => {
    try {
      const response = await fetch('/api/rules')
      const payload = await response.json()
      if (payload.success && payload.data) setRules(payload.data)
    } catch {
      // Keep the last known state during transient failures.
    }
  }, [])

  return { rules, fetchRules }
}

export function useVersion() {
  const [versionInfo, setVersionInfo] = useState({
    current: '',
    latest: null,
    has_update: false,
    upgrade_supported: true,
  })

  const fetchVersion = useCallback(async () => {
    try {
      const response = await fetch('/api/version')
      const payload = await response.json()
      if (payload.success && payload.data) {
        setVersionInfo(payload.data)
        return payload.data
      }
    } catch {
      // Keep the last known state during transient failures.
    }
    return null
  }, [])

  return { versionInfo, fetchVersion }
}
