import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useToast, useApi } from './useApi'
import { useStatus, useSubs, useNodes, useRules, useVersion } from './useResources'
import {
  collectNodeProtocols,
  collectRuleTargetNodes,
  mergeDisplayMeasurements,
  useConnections,
  useDelays,
  useProxies,
  useTraffic,
} from './useClash'
import { usePolling, type PollTask } from './usePolling'
import { useDesktopLayout } from './useDesktopLayout'
import {
  CLASH_API_BASE,
  CONFIG_POLL_INTERVAL,
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

export interface OnboardingGateInput {
  firstLoadDone: boolean
  statusLoaded: boolean
  backendUnreachable: boolean
  initializing: boolean
  ready: boolean
  subsCurrent: boolean
  nodesCurrent: boolean
  rulesCurrent: boolean
  subsAvailable: boolean
  nodesAvailable: boolean
  subsError: string
  nodesError: string
  rulesError: string
  subsCount: number
  nodesCount: number
}

/**
 * A later successful read can complete startup even if the first request failed.
 * When `/api/status` has settled without ever succeeding, config polling is still
 * disabled, so we settle immediately to show the backend-reconnecting state.
 */
export function isInitialLoadComplete(state: {
  statusSettled: boolean
  statusLoaded: boolean
  subsSettled: boolean
  nodesSettled: boolean
  rulesSettled: boolean
}): boolean {
  if (!state.statusSettled) return false
  if (!state.statusLoaded) return true
  return state.subsSettled && state.nodesSettled && state.rulesSettled
}

/**
 * Determines whether to show onboarding instead of the dashboard.
 * Every guard prevents a distinct false-positive empty-config state:
 * - unreachable backend or unsettled initial wave
 * - active or still-initializing runtime
 * - config projections lagging behind `status.data_revision`
 * - failed list reads whose placeholder `[]` is not authoritative emptiness
 */
export function shouldEnterOnboarding(input: OnboardingGateInput): boolean {
  if (!input.firstLoadDone || !input.statusLoaded || input.backendUnreachable) return false
  if (input.initializing || input.ready) return false
  if (!input.subsCurrent || !input.nodesCurrent || !input.rulesCurrent) return false
  if (!input.subsAvailable || !input.nodesAvailable) return false
  if (input.subsError || input.nodesError || input.rulesError) return false
  return input.subsCount === 0 && input.nodesCount === 0
}

/**
 * Auto-test the active node once per node selection in manual (`Selector`) mode.
 * `URLTest` (`fastest_*`) groups are excluded because probing a single node in Clash API
 * triggers immediate `PerformUpdateCheck` against stale peers and causes rapid flapping.
 */
export function shouldAutoTestCurrentNode(state: {
  ready: boolean
  initializing: boolean
  currentNodeName: string
  isUrlTestGroup: boolean
  lastAutoTestedNode: string
}): boolean {
  if (!state.ready || state.initializing || !state.currentNodeName) return false
  if (state.isUrlTestGroup) return false
  return state.currentNodeName !== state.lastAutoTestedNode
}

export function useAppData() {
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
  const { status, statusLoaded, statusFailures, statusSettled, fetchStatus } = useStatus()
  const revision = status.data_revision ?? 0
  const { subs, subsLoaded, subsAvailable, subsCurrent, subsError, fetchSubs } = useSubs(revision)
  const { nodes, nodesLoaded, nodesAvailable, nodesCurrent, nodesError, fetchNodes } = useNodes(revision)
  const { rules, rulesLoaded, rulesCurrent, rulesError, fetchRules } = useRules(revision)
  const { proxies, primaryGroupName, primaryGroup, fetchProxies } = useProxies(status)

  const refreshAfterUncertainWrite = useCallback(() => {
    void Promise.allSettled([fetchStatus(), fetchSubs(), fetchNodes(), fetchRules(), fetchProxies()])
  }, [fetchStatus, fetchSubs, fetchNodes, fetchRules, fetchProxies])
  const { apiCall, pendingActions } = useApi(refreshAfterUncertainWrite)

  const nodeProtocols = useMemo(() => collectNodeProtocols(proxies), [proxies])
  const ruleNodeNames = useMemo(() => collectRuleTargetNodes(nodes, proxies), [nodes, proxies])
  const { traffic, closeSockets } = useTraffic(status)
  const {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
  } = useConnections(status, clashApiBase)
  const { versionInfo, fetchVersion } = useVersion()
  const { delays, delayMeasuredAt, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays } = useDelays()

  const autoTestedNodeRef = useRef('')
  const currentNodeName = primaryGroup?.now || ''
  const isUrlTestGroup = primaryGroup?.type === 'URLTest'
  useEffect(() => {
    const shouldAutoTest = shouldAutoTestCurrentNode({
      ready: status.ready,
      initializing: status.initializing,
      currentNodeName,
      isUrlTestGroup,
      lastAutoTestedNode: autoTestedNodeRef.current,
    })
    if (!shouldAutoTest) return
    autoTestedNodeRef.current = currentNodeName
    testDelay(clashApiBase, currentNodeName)
  }, [status.ready, status.initializing, currentNodeName, isUrlTestGroup, clashApiBase, testDelay])

  const displayMeasurements = useMemo(
    () => mergeDisplayMeasurements({ proxies, isUrlTestGroup, delays, delayMeasuredAt }),
    [proxies, isUrlTestGroup, delays, delayMeasuredAt],
  )

  const resetNodeForm = useCallback(() => {
    setNodeType('hysteria2')
    setNodeForm({ ...EMPTY_NODE_FORM, ...nodeTypeDefaults('hysteria2') })
  }, [])

  const firstLoadDone = isInitialLoadComplete({
    statusSettled,
    statusLoaded,
    subsSettled: subsLoaded,
    nodesSettled: nodesLoaded,
    rulesSettled: rulesLoaded,
  })

  // 连续失败达到阈值视为后端不可达：面板显示断线提示，且不再按空数据误判进入引导页
  const backendUnreachable = statusFailures >= STATUS_FAILURE_THRESHOLD

  const needsOnboarding = shouldEnterOnboarding({
    firstLoadDone,
    statusLoaded,
    backendUnreachable,
    initializing: status.initializing,
    ready: status.ready,
    subsCurrent,
    nodesCurrent,
    rulesCurrent,
    subsAvailable,
    nodesAvailable,
    subsError,
    nodesError,
    rulesError,
    subsCount: subs.length,
    nodesCount: nodes.length,
  })

  const pollingTasks = useMemo<PollTask[]>(() => {
    const tasks: PollTask[] = [fetchStatus]
    if (status.ready) {
      tasks.push(fetchProxies)
    }
    return tasks
  }, [fetchStatus, fetchProxies, status.ready])

  // ready 由 false 变为 true 时立即补取 Clash 数据，不能等下一轮常规轮询；
  // 这样后端提前展示面板后，数据面一就绪节点列表就会立刻出现。
  useEffect(() => {
    if (status.ready) fetchProxies()
  }, [status.ready, fetchProxies])

  const connectionPollingTasks = useMemo(() => [fetchConnections], [fetchConnections])

  const configTasks = useMemo(() => [fetchSubs, fetchNodes, fetchRules], [fetchSubs, fetchNodes, fetchRules])
  const lastRevision = useRef<number | undefined>(undefined)
  useEffect(() => {
    if (!statusLoaded) return
    if (lastRevision.current !== undefined && lastRevision.current !== revision) {
      configTasks.forEach(task => { void task() })
    }
    lastRevision.current = revision
  }, [statusLoaded, revision, configTasks])

  usePolling(configTasks, statusLoaded, CONFIG_POLL_INTERVAL)
  usePolling(pollingTasks)
  usePolling(
    connectionPollingTasks,
    Boolean(status.ready) && (showConnectionsModal || isDesktop || rules.length > 0),
  )

  useEffect(() => {
    fetchVersion()
  }, [fetchVersion])

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
    pendingActions,
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
    subsError,
    fetchSubs,
    nodes,
    nodesError,
    fetchNodes,
    rules,
    rulesError,
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
    delays: displayMeasurements.delays,
    delayMeasuredAt: displayMeasurements.measuredAt,
    testingNodes,
    testingGroup,
    testDelay,
    testGroupDelays,
    clearDelays,
    resetNodeForm,
    needsOnboarding,
  }
}
