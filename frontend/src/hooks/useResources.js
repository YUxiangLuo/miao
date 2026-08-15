import { useCallback, useState } from 'react'

export function useStatus() {
  const [status, setStatus] = useState({
    running: false,
    pid: null,
    uptime_secs: null,
    initializing: false,
    route_mode: 'rule',
    adblock: false
  })

  const fetchStatus = useCallback(async () => {
    try {
      const response = await fetch('/api/status')
      const payload = await response.json()
      if (payload.success && payload.data) setStatus(payload.data)
    } catch {
      // Keep the last known state during transient failures.
    }
  }, [])

  return { status, fetchStatus }
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
      // Keep the last known state during transient failures.
    }
    return null
  }, [])

  return { versionInfo, fetchVersion }
}
