import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useToast, useApi } from './useApi'
import { useStatus, useSubs, useNodes, useRules, useVersion } from './useResources'
import { useProxies, useTraffic, useConnections, useDelays, isClashProxyGroup } from './useClash'
import { usePolling, type PollTask } from './usePolling'
import { useDesktopLayout } from './useDesktopLayout'
import {
  CLASH_API_BASE,
  EMPTY_NODE_FORM,
  nodeTypeDefaults,
  STATUS_FAILURE_THRESHOLD,
  type NodeForm,
} from '../utils'
import type { NodeType } from '../types/api'

/** 确认对话框状态（ConfirmModal 的受控数据源） */
export interface ConfirmState {
  open: boolean
  title: string
  message: string
  onConfirm: (() => void) | null
}

export function useAppData() {
  const [firstLoadDone, setFirstLoadDone] = useState(false)
  const [loadingAction, setLoadingAction] = useState('')
  const [upgrading, setUpgrading] = useState(false)
  const [nodeForm, setNodeForm] = useState<NodeForm>(EMPTY_NODE_FORM)
  const [nodeType, setNodeType] = useState<NodeType>('hysteria2')
  const [showNodeModal, setShowNodeModal] = useState(false)
  const [showConnectionsModal, setShowConnectionsModal] = useState(false)
  const [confirmState, setConfirmState] = useState<ConfirmState>({ open: false, title: '', message: '', onConfirm: null })
  const [switchingNode, setSwitchingNode] = useState('')

  const isDesktop = useDesktopLayout()
  const clashApiBase = CLASH_API_BASE

  const { toasts, showToast, dismissToast } = useToast()
  const { apiCall } = useApi({ setLoadingAction })
  const { status, statusLoaded, statusFailures, fetchStatus } = useStatus()
  const { subs, fetchSubs } = useSubs()
  const { nodes, fetchNodes } = useNodes()
  const { rules, fetchRules } = useRules()
  const { proxies, primaryGroupName, primaryGroup, fetchProxies } = useProxies(status)

  // 节点名 → 协议类型（Clash API 的 type，如 Hysteria2/AnyTLS/VLESS）；分组项不入图
  const nodeProtocols = useMemo(() => {
    const map: Record<string, string> = {}
    Object.entries(proxies || {}).forEach(([name, proxy]) => {
      if (proxy?.type && !isClashProxyGroup(proxy.type)) map[name] = proxy.type
    })
    return map
  }, [proxies])

  // 规则「指定节点」下拉的候选:手动节点(服务停止时也在) ∪ 运行时全部 outbound
  // 与后端 known_rule_targets 同口径(排除内置 proxy/direct 与分组项),不随 fastest_* 地区过滤收缩
  const ruleNodeNames = useMemo(() => {
    const names = new Set<string>(nodes.map((node) => node.tag))
    Object.entries(proxies || {}).forEach(([name, proxy]) => {
      if (name !== 'proxy' && name !== 'direct' && !isClashProxyGroup(proxy?.type)) {
        names.add(name)
      }
    })
    return [...names]
  }, [nodes, proxies])
  const { traffic, closeSockets } = useTraffic(status)
  const {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
  } = useConnections(status, clashApiBase)
  const { versionInfo, fetchVersion } = useVersion()
  const { delays, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays } = useDelays()

  // 进入首页且当前节点就绪后,自动测一次延迟;切换节点后也会测新节点。
  // 每个节点每次会话只自动测一次,手动点测不受影响
  const autoTestedNodeRef = useRef('')
  const currentNodeName = primaryGroup?.now || ''
  useEffect(() => {
    if (!status.ready || status.initializing || !currentNodeName) return
    if (currentNodeName === autoTestedNodeRef.current) return
    autoTestedNodeRef.current = currentNodeName
    testDelay(clashApiBase, currentNodeName)
  }, [status.ready, status.initializing, currentNodeName, clashApiBase, testDelay])

  const resetNodeForm = useCallback(() => {
    setNodeType('hysteria2')
    setNodeForm({ ...EMPTY_NODE_FORM, ...nodeTypeDefaults('hysteria2') })
  }, [])

  // 首次加载：获取初始状态后再决定显示 onboarding 还是 dashboard
  // Clash API 不属于首屏关键路径：内核未就绪或 API 卡顿时不能拖住整个面板。
  // status/subs/nodes/rules 都由 miao 本身直接提供，足够决定首屏结构。
  useEffect(() => {
    Promise.all([fetchStatus(), fetchSubs(), fetchNodes(), fetchRules()])
      .finally(() => setFirstLoadDone(true))
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // 连续失败达到阈值视为后端不可达：面板显示断线提示，且不再按空数据误判进入引导页
  const backendUnreachable = statusFailures >= STATUS_FAILURE_THRESHOLD

  const needsOnboarding = firstLoadDone
    && statusLoaded
    && !backendUnreachable
    && !status.initializing
    && !status.ready
    && subs.length === 0
    && nodes.length === 0

  const pollingTasks = useMemo<PollTask[]>(() => {
    const tasks: PollTask[] = [fetchStatus, fetchSubs, fetchNodes, fetchRules]
    if (status.ready) {
      tasks.push(fetchProxies)
    }
    return tasks
  }, [fetchStatus, fetchSubs, fetchNodes, fetchRules, fetchProxies, status.ready])

  // ready 由 false 变为 true 时立即补取 Clash 数据，不能等下一轮常规轮询；
  // 这样后端提前展示面板后，数据面一就绪节点列表就会立刻出现。
  useEffect(() => {
    if (status.ready) fetchProxies()
  }, [status.ready, fetchProxies])

  const connectionPollingTasks = useMemo(() => [fetchConnections], [fetchConnections])

  usePolling(pollingTasks)
  usePolling(
    connectionPollingTasks,
    Boolean(status.ready) && (showConnectionsModal || isDesktop || rules.length > 0),
  )

  useEffect(() => {
    fetchVersion()
  }, [status.ready, fetchVersion])

  useEffect(() => {
    return () => closeSockets()
  }, [closeSockets])

  useEffect(() => {
    if (status.warning) {
      showToast(status.warning, 'error')
    }
  }, [status.warning, showToast])

  useEffect(() => {
    if (!status.ready) {
      clearDelays()
    }
  }, [status.ready, clearDelays])

  useEffect(() => {
    if (!isDesktop) setShowConnectionsModal(false)
  }, [isDesktop])

  return {
    firstLoadDone,
    loadingAction,
    upgrading,
    setUpgrading,
    nodeForm,
    setNodeForm,
    nodeType,
    setNodeType,
    showNodeModal,
    setShowNodeModal,
    showConnectionsModal,
    setShowConnectionsModal,
    confirmState,
    setConfirmState,
    switchingNode,
    setSwitchingNode,
    isDesktop,
    clashApiBase,
    toasts,
    showToast,
    dismissToast,
    apiCall,
    status,
    statusLoaded,
    backendUnreachable,
    fetchStatus,
    subs,
    fetchSubs,
    nodes,
    fetchNodes,
    rules,
    fetchRules,
    primaryGroupName,
    primaryGroup,
    fetchProxies,
    ruleNodeNames,
    nodeProtocols,
    traffic,
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
    versionInfo,
    fetchVersion,
    delays,
    testingNodes,
    testingGroup,
    testDelay,
    testGroupDelays,
    clearDelays,
    resetNodeForm,
    needsOnboarding,
  }
}
