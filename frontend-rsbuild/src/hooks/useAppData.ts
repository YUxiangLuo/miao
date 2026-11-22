import { createEffect, createMemo, createSignal, onCleanup, onSettled } from 'solid-js'
import { useToast, useApi } from './useApi'
import { useStatus, useSubs, useNodes, useRules, useVersion, useTerminals, useAccessPorts } from './useResources'
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

export type ConsoleNavKey =
  | 'overview'
  | 'nodes'
  | 'rules'
  | 'lan'
  | 'connections'
  | 'settings'

export type NodeModalInitialMode = 'link' | 'manual' | 'vps'

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

export function shouldEnterOnboarding(input: OnboardingGateInput): boolean {
  if (!input.firstLoadDone || !input.statusLoaded || input.backendUnreachable) return false
  if (input.initializing || input.ready) return false
  if (!input.subsCurrent || !input.nodesCurrent || !input.rulesCurrent) return false
  if (!input.subsAvailable || !input.nodesAvailable) return false
  if (input.subsError || input.nodesError || input.rulesError) return false
  return input.subsCount === 0 && input.nodesCount === 0
}

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
  const [activeNav, setActiveNav] = createSignal<ConsoleNavKey>('overview')
  const [upgrading, setUpgrading] = createSignal(false)
  const [nodeForm, setNodeForm] = createSignal<NodeForm>({ ...EMPTY_NODE_FORM })
  const [nodeType, setNodeType] = createSignal<NodeType>('hysteria2')
  const [nodeModalMode, setNodeModalMode] = createSignal<NodeModalInitialMode>('link')
  const [showNodeModal, setShowNodeModal] = createSignal(false)
  const [showConnectionsModal, setShowConnectionsModal] = createSignal(false)
  const [confirmState, setConfirmState] = createSignal<ConfirmState>({
    open: false,
    title: '',
    message: '',
    onConfirm: null,
  })
  const [switchingNode, setSwitchingNode] = createSignal('')

  const isDesktop = useDesktopLayout()
  const clashApiBase = CLASH_API_BASE

  const { toasts, showToast, dismissToast } = useToast()
  const { status, statusLoaded, statusFailures, statusSettled, fetchStatus } = useStatus()
  const revision = createMemo(() => status().data_revision ?? 0)
  const isReady = createMemo(() => Boolean(status().ready))
  const { subs, subsLoaded, subsAvailable, subsCurrent, subsError, fetchSubs } = useSubs(revision)
  const { nodes, nodesLoaded, nodesAvailable, nodesCurrent, nodesError, fetchNodes } = useNodes(revision)
  const { rules, rulesLoaded, rulesCurrent, rulesError, fetchRules } = useRules(revision)
  const {
    terminals,
    terminalsSupported,
    terminalsError,
    fetchTerminals,
  } = useTerminals(revision)
  const {
    accessPorts,
    accessPortsSupported,
    accessPortsError,
    fetchAccessPorts,
  } = useAccessPorts(revision)
  const { proxies, primaryGroupName, primaryGroup, fetchProxies } = useProxies(isReady)

  const refreshAfterUncertainWrite = () => {
    void Promise.allSettled([
      fetchStatus(),
      fetchSubs(),
      fetchNodes(),
      fetchRules(),
      fetchTerminals(),
      fetchAccessPorts(),
      fetchProxies(),
    ])
  }
  const { apiCall, pendingActions } = useApi(refreshAfterUncertainWrite)

  const nodeProtocols = createMemo(() => collectNodeProtocols(proxies()))
  const ruleNodeNames = createMemo(() => collectRuleTargetNodes(nodes(), proxies()))
  const { traffic, closeSockets } = useTraffic(isReady)
  const {
    connectionsInfo,
    connectionsLoading,
    connectionsError,
    fetchConnections,
  } = useConnections(isReady, clashApiBase)
  const { versionInfo, fetchVersion } = useVersion()
  const { delays, delayMeasuredAt, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays } = useDelays()

  let lastAutoTestedNode = ''
  const currentNodeName = createMemo(() => primaryGroup()?.now || '')
  const isUrlTestGroup = createMemo(() => primaryGroup()?.type === 'URLTest')

  createEffect(
    () => ({
      ready: status().ready,
      initializing: status().initializing,
      node: currentNodeName(),
      urlTest: isUrlTestGroup(),
    }),
    ({ ready, initializing, node, urlTest }) => {
      const shouldAutoTest = shouldAutoTestCurrentNode({
        ready,
        initializing,
        currentNodeName: node,
        isUrlTestGroup: urlTest,
        lastAutoTestedNode,
      })
      if (!shouldAutoTest) return
      lastAutoTestedNode = node
      void testDelay(clashApiBase, node)
    },
  )

  const displayMeasurements = createMemo(() => mergeDisplayMeasurements({
    proxies: proxies(),
    isUrlTestGroup: isUrlTestGroup(),
    delays: delays(),
    delayMeasuredAt: delayMeasuredAt(),
  }))
  const mergedDelays = createMemo(() => displayMeasurements().delays)
  const mergedMeasuredAt = createMemo(() => displayMeasurements().measuredAt)

  const resetNodeForm = () => {
    setNodeType('hysteria2')
    setNodeForm({ ...EMPTY_NODE_FORM, ...nodeTypeDefaults('hysteria2') })
  }

  const firstLoadDone = createMemo(() => isInitialLoadComplete({
    statusSettled: statusSettled(),
    statusLoaded: statusLoaded(),
    subsSettled: subsLoaded(),
    nodesSettled: nodesLoaded(),
    rulesSettled: rulesLoaded(),
  }))

  const backendUnreachable = createMemo(() => statusFailures() >= STATUS_FAILURE_THRESHOLD)

  const needsOnboarding = createMemo(() => shouldEnterOnboarding({
    firstLoadDone: firstLoadDone(),
    statusLoaded: statusLoaded(),
    backendUnreachable: backendUnreachable(),
    initializing: status().initializing,
    ready: status().ready,
    subsCurrent: subsCurrent(),
    nodesCurrent: nodesCurrent(),
    rulesCurrent: rulesCurrent(),
    subsAvailable: subsAvailable(),
    nodesAvailable: nodesAvailable(),
    subsError: subsError(),
    nodesError: nodesError(),
    rulesError: rulesError(),
    subsCount: subs().length,
    nodesCount: nodes().length,
  }))

  const pollingTasks = createMemo<PollTask[]>(() => {
    const tasks: PollTask[] = [fetchStatus]
    if (isReady()) tasks.push(fetchProxies)
    return tasks
  })

  createEffect(
    () => isReady(),
    (ready) => {
      if (ready) void fetchProxies()
      else clearDelays()
    },
  )

  const configTasks: PollTask[] = [fetchSubs, fetchNodes, fetchRules, fetchTerminals, fetchAccessPorts]
  let lastRevision: number | undefined
  createEffect(
    () => ({ loaded: statusLoaded(), rev: revision() }),
    ({ loaded, rev }) => {
      if (!loaded) return
      if (lastRevision !== undefined && lastRevision !== rev) {
        configTasks.forEach((task) => { void task() })
      }
      lastRevision = rev
    },
  )

  usePolling(() => configTasks, statusLoaded, CONFIG_POLL_INTERVAL)
  usePolling(pollingTasks)
  usePolling(
    () => [fetchConnections],
    () => isReady() && (showConnectionsModal() || isDesktop() || rules().length > 0 || accessPorts().length > 0),
  )

  onSettled(() => {
    void fetchVersion()
  })

  onCleanup(() => closeSockets())

  createEffect(
    () => status().warning,
    (warning) => {
      if (warning) showToast(warning, 'error')
    },
  )

  createEffect(
    () => isDesktop(),
    (desktop) => {
      if (!desktop) setShowConnectionsModal(false)
    },
  )

  return {
    activeNav,
    setActiveNav,
    firstLoadDone,
    pendingActions,
    upgrading,
    setUpgrading,
    nodeForm,
    setNodeForm,
    nodeType,
    setNodeType,
    nodeModalMode,
    setNodeModalMode,
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
    terminals,
    terminalsSupported,
    terminalsError,
    fetchTerminals,
    accessPorts,
    accessPortsSupported,
    accessPortsError,
    fetchAccessPorts,
    proxies,
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
    delays: mergedDelays,
    delayMeasuredAt: mergedMeasuredAt,
    testingNodes,
    testingGroup,
    testDelay,
    testGroupDelays,
    clearDelays,
    resetNodeForm,
    needsOnboarding,
  }
}
