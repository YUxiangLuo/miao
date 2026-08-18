import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useToast, useApi } from './useApi.js'
import { useStatus, useSubs, useNodes, useRules, useVersion } from './useResources.js'
import { useProxies, useTraffic, useConnections, useDelays, isClashProxyGroup } from './useClash.js'
import { usePolling } from './usePolling.js'
import { useDesktopLayout } from './useDesktopLayout.js'
import { EMPTY_NODE_FORM, nodeTypeDefaults, POLL_INTERVAL, POLL_INTERVAL_STARTUP, STATUS_FAILURE_THRESHOLD } from '../utils.js'

export function useAppData() {
  const [firstLoadDone, setFirstLoadDone] = useState(false)
  const [loadingAction, setLoadingAction] = useState('')
  const [upgrading, setUpgrading] = useState(false)
  const [newSubUrl, setNewSubUrl] = useState('')
  const [nodeForm, setNodeForm] = useState(EMPTY_NODE_FORM)
  const [nodeType, setNodeType] = useState('hysteria2')
  const [showNodeModal, setShowNodeModal] = useState(false)
  const [showConnectionsModal, setShowConnectionsModal] = useState(false)
  const [confirmState, setConfirmState] = useState({ open: false, title: '', message: '', onConfirm: null })
  const [switchingNode, setSwitchingNode] = useState('')

  const isDesktop = useDesktopLayout()
  const clashApiBase = useMemo(() => '/api/clash', [])

  const { toasts, showToast, dismissToast } = useToast()
  const { apiCall } = useApi({ setLoadingAction })
  const { status, statusLoaded, statusFailures, fetchStatus } = useStatus()
  const { subs, fetchSubs } = useSubs()
  const { nodes, fetchNodes } = useNodes()
  const { rules, fetchRules } = useRules()
  const { proxies, primaryGroupName, primaryGroup, fetchProxies } = useProxies(status)

  // 规则「指定节点」下拉的候选:手动节点(服务停止时也在) ∪ 运行时全部 outbound
  // 与后端 known_rule_targets 同口径(排除内置 proxy/direct 与分组项),不随 fastest_* 地区过滤收缩
  const ruleNodeNames = useMemo(() => {
    const names = new Set(nodes.map((node) => node.tag))
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

  const nodeMetaMap = useMemo(() => {
    const map = new Map()
    nodes.forEach((node) => map.set(node.tag, node))
    return map
  }, [nodes])

  const currentNodeMeta = primaryGroup?.now ? nodeMetaMap.get(primaryGroup.now) : null

  // 进入首页且当前节点就绪后,自动测一次延迟;切换节点后也会测新节点。
  // 每个节点每次会话只自动测一次,手动点测不受影响
  const autoTestedNodeRef = useRef('')
  const currentNodeName = primaryGroup?.now || ''
  useEffect(() => {
    if (!status.running || status.initializing || !currentNodeName) return
    if (currentNodeName === autoTestedNodeRef.current) return
    autoTestedNodeRef.current = currentNodeName
    testDelay(clashApiBase, currentNodeName)
  }, [status.running, status.initializing, currentNodeName, clashApiBase, testDelay])

  const resetNodeForm = useCallback(() => {
    setNodeType('hysteria2')
    setNodeForm({ ...EMPTY_NODE_FORM, ...nodeTypeDefaults('hysteria2') })
  }, [])

  // 首次加载：获取初始状态后再决定显示 onboarding 还是 dashboard
  // 同时拉取代理组，避免服务运行时首屏短暂显示“等待服务启动”
  useEffect(() => {
    Promise.all([fetchStatus(), fetchSubs(), fetchNodes(), fetchRules(), fetchProxies()])
      .finally(() => setFirstLoadDone(true))
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // 连续失败达到阈值视为后端不可达：面板显示断线提示，且不再按空数据误判进入引导页
  const backendUnreachable = statusFailures >= STATUS_FAILURE_THRESHOLD

  const needsOnboarding = firstLoadDone
    && statusLoaded
    && !backendUnreachable
    && !status.initializing
    && !status.running
    && subs.length === 0
    && nodes.length === 0

  const pollingTasks = useMemo(() => {
    const tasks = [fetchStatus, fetchSubs, fetchNodes, fetchRules]
    if (status.running) {
      tasks.push(fetchProxies)
    }
    return tasks
  }, [fetchStatus, fetchSubs, fetchNodes, fetchRules, fetchProxies, status.running])

  const connectionPollingTasks = useMemo(() => [fetchConnections], [fetchConnections])

  // 初始化期（内核正在拉起）加速轮询，就绪状态近乎即时呈现；平时 3s
  usePolling(pollingTasks, true, status.initializing ? POLL_INTERVAL_STARTUP : POLL_INTERVAL)
  usePolling(
    connectionPollingTasks,
    status.running && (showConnectionsModal || isDesktop || rules.length > 0),
  )

  useEffect(() => {
    fetchVersion()
  }, [status.running, fetchVersion])

  useEffect(() => {
    return () => closeSockets()
  }, [closeSockets])

  useEffect(() => {
    if (status.warning) {
      showToast(status.warning, 'error')
    }
  }, [status.warning, showToast])

  useEffect(() => {
    if (!status.running) {
      clearDelays()
    }
  }, [status.running, clearDelays])

  useEffect(() => {
    if (!isDesktop) setShowConnectionsModal(false)
  }, [isDesktop])

  return {
    firstLoadDone,
    loadingAction,
    upgrading,
    setUpgrading,
    newSubUrl,
    setNewSubUrl,
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
    currentNodeMeta,
    resetNodeForm,
    needsOnboarding,
  }
}
