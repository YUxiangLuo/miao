import { useCallback, useState } from 'react'
import type { ApiResponse, NodeInfo, RuleInfo, StatusData, SubStatus, VersionInfo } from '../types/api'

// 未拿到首个响应前的占位状态。vps_supported: true 与历史上的 undefined 等价
// （消费方判断 `!== false`）；platform 兜底与 ruleFieldOptions 的默认参数一致。
const INITIAL_STATUS: StatusData = {
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

export function useStatus() {
  const [status, setStatus] = useState<StatusData>(INITIAL_STATUS)
  // 是否成功拿到过后端响应：区分「服务未运行」与「后端根本没起来」
  const [statusLoaded, setStatusLoaded] = useState(false)
  // 连续失败次数（成功即清零），作为面板断线提示的健康度信号
  const [statusFailures, setStatusFailures] = useState(0)

  const fetchStatus = useCallback(async () => {
    try {
      const response = await fetch('/api/status')
      const payload: ApiResponse<StatusData> = await response.json()
      if (payload.success && payload.data) {
        const ready = payload.data.ready ?? (payload.data.running && !payload.data.initializing)
        const phase = payload.data.phase ?? (ready ? 'ready' : payload.data.initializing ? 'initializing' : 'stopped')
        setStatus({ ...payload.data, ready, phase })
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
  const [subs, setSubs] = useState<SubStatus[]>([])

  const fetchSubs = useCallback(async () => {
    try {
      const response = await fetch('/api/subs')
      const payload: ApiResponse<SubStatus[]> = await response.json()
      if (payload.success && payload.data) setSubs(payload.data)
    } catch {
      // Keep the last known state during transient failures.
    }
  }, [])

  return { subs, fetchSubs }
}

export function useNodes() {
  const [nodes, setNodes] = useState<NodeInfo[]>([])

  const fetchNodes = useCallback(async () => {
    try {
      const response = await fetch('/api/nodes')
      const payload: ApiResponse<NodeInfo[]> = await response.json()
      if (payload.success && payload.data) setNodes(payload.data)
    } catch {
      // Keep the last known state during transient failures.
    }
  }, [])

  return { nodes, fetchNodes }
}

export function useRules() {
  const [rules, setRules] = useState<RuleInfo[]>([])

  const fetchRules = useCallback(async () => {
    try {
      const response = await fetch('/api/rules')
      const payload: ApiResponse<RuleInfo[]> = await response.json()
      if (payload.success && payload.data) setRules(payload.data)
    } catch {
      // Keep the last known state during transient failures.
    }
  }, [])

  return { rules, fetchRules }
}

const INITIAL_VERSION: VersionInfo = {
  current: '',
  latest: null,
  has_update: false,
  download_url: null,
  upgrade_supported: true,
}

export function useVersion() {
  const [versionInfo, setVersionInfo] = useState<VersionInfo>(INITIAL_VERSION)

  const fetchVersion = useCallback(async (): Promise<VersionInfo | null> => {
    try {
      const response = await fetch('/api/version')
      const payload: ApiResponse<VersionInfo> = await response.json()
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
