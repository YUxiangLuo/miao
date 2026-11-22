import { createMemo, createSignal } from 'solid-js'
import { CONNECTIONS_MODAL_MIN_WIDTH } from '../tokens'
import { validateSubscriptionUrl } from '../utils'
import { buildNodeRequest } from '../nodeForm'
import { describeVersionCheckResult, waitForUpgrade } from './upgrade'
import { isUncertainRequestError } from './request'
import { useVpsDeployment } from './useVpsDeployment'
import {
  batchNodeFailuresFeedback,
  mutationFeedback,
  nodeSelectFeedback,
  routeModeConfirmCopy,
  subscriptionRefreshFeedback,
  switchProxyFeedback,
  vergeImportFeedback,
  type ToastFeedback,
} from './mutationFeedback'
import type { ResourceReadResult } from './useResources'
import type { ToastTone } from './useApi'
import type { useAppData } from './useAppData'
import type {
  AccessPortStatus,
  AssignTerminalRequest,
  DeleteAccessPortRequest,
  DeleteTerminalRequest,
  NodeRequest,
  RouteModeRequest,
  MaxMultiplierRequest,
  NodeSelectRequest,
  NodeSelectResult,
  RouteModeResult,
  MaxMultiplierResult,
  LastProxy,
  SubRequest,
  SubBatchRequest,
  SetNodeDisabledRequest,
  BatchNodeRequest,
  DeleteNodeRequest,
  DeleteRuleRequest,
  McpRequest,
  SaveAccessPortRequest,
  SwitchProxyResult,
  BatchNodeResult,
  NodeSelect,
  RouteMode,
  RuleInfo,
  RuleRequest,
  ScheduledRefreshRequest,
  SubBatchResult,
  TerminalSummary,
  UpsertTerminalRequest,
  VergeImportResult,
  SubscriptionRefreshResult,
} from '../types/api'

type AppData = ReturnType<typeof useAppData>

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function useAppActions(data: AppData) {
  const { vpsTasks, vpsDeploying, handleDeployVps } = useVpsDeployment(data)
  const [upgradeMessage, setUpgradeMessage] = createSignal('')
  const deleting = new Set<string>()
  const [pendingDeletes, setPendingDeletes] = createSignal<ReadonlySet<string>>(new Set())

  const requestedNodeSelect = () => data.status().requested_node_select || data.status().node_select || 'manual'

  const presentFeedback = (feedback: ToastFeedback | null) => {
    if (feedback) data.showToast(feedback.message, feedback.tone)
  }

  const showMutationResult = (
    message: string,
    read: ResourceReadResult<unknown>,
    action: string,
    tone?: ToastTone,
  ) => {
    presentFeedback(mutationFeedback(message, read, action, tone))
  }

  const openConfirm = (title: string, message: string, onConfirm: () => void) => {
    data.setConfirmState({ open: true, title, message, onConfirm })
  }

  const closeConfirm = () => {
    data.setConfirmState({ open: false, title: '', message: '', onConfirm: null })
  }

  const openNodeModal = (preset?: 'link' | 'manual' | 'socks' | 'vps') => {
    if (data.status().initializing) {
      data.showToast('初始化完成后才能修改节点', 'info')
      return
    }
    if (preset === 'socks') {
      data.setNodeType('socks')
      data.setNodeForm((prev) => ({
        ...prev,
        server_port: 1080,
        tls_enabled: false,
      }))
      data.setNodeModalMode('manual')
    } else if (preset === 'manual') {
      data.setNodeModalMode('manual')
    } else if (preset === 'vps') {
      data.setNodeModalMode('vps')
    } else {
      data.setNodeModalMode('link')
    }
    data.setShowNodeModal(true)
  }

  const closeNodeModal = () => {
    data.setShowNodeModal(false)
    data.resetNodeForm()
  }

  const handleSetRouteMode = async (nextMode: RouteMode) => {
    if (nextMode === data.status().route_mode) return

    try {
      await data.apiCall<RouteModeResult>(
        'route-mode',
        { method: 'POST', body: JSON.stringify({ route_mode: nextMode } satisfies RouteModeRequest) },
        'routeMode',
      )
      data.clearDelays()
      await data.fetchStatus()
      await data.fetchProxies()
      data.showToast(nextMode === 'global' ? '已切换为全局代理' : '已切换为规则分流', 'success')
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleOpenSetRouteModeConfirm = (nextMode: RouteMode) => {
    if (nextMode === data.status().route_mode) return
    const copy = routeModeConfirmCopy(nextMode)
    openConfirm(copy.title, copy.message, () => { void handleSetRouteMode(nextMode) })
  }

  const handleSetMaxMultiplier = async (nextMultiplier: string | null) => {
    if (nextMultiplier === (data.status().max_multiplier ?? null)) return

    try {
      await data.apiCall<MaxMultiplierResult>(
        'max-multiplier',
        { method: 'POST', body: JSON.stringify({ max_multiplier: nextMultiplier } satisfies MaxMultiplierRequest) },
        'maxMultiplier',
      )
      data.clearDelays()
      await data.fetchStatus()
      await data.fetchProxies()
      data.showToast(nextMultiplier === null ? '已取消倍率限制' : `最高倍率已设为 ${nextMultiplier}x`, 'success')
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleSetNodeSelect = async (nextSelect: NodeSelect) => {
    if (nextSelect === requestedNodeSelect()) return

    try {
      const payload = await data.apiCall<NodeSelectResult>(
        'node-select',
        { method: 'POST', body: JSON.stringify({ node_select: nextSelect } satisfies NodeSelectRequest) },
        'nodeSelect',
      )
      data.clearDelays()
      const read = await data.fetchStatus()
      await data.fetchProxies()
      presentFeedback(nodeSelectFeedback(payload.data, read))
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleSwitchProxy = async (groupName: string, nodeName: string) => {
    if (requestedNodeSelect() !== 'manual') return
    if (data.switchingNode()) return
    data.setSwitchingNode(nodeName)
    try {
      const response = await data.apiCall<SwitchProxyResult>('proxy/switch', {
        method: 'POST',
        body: JSON.stringify({ group: groupName, name: nodeName } satisfies LastProxy),
      }, 'switchProxy')
      await data.fetchProxies()
      presentFeedback(switchProxyFeedback(nodeName, response.data))
    } catch (error) {
      data.showToast(errorMessage(error) || '切换节点失败', 'error')
    } finally {
      data.setSwitchingNode('')
    }
  }

  const handleAddSubscription = async (url: string): Promise<boolean> => {
    const trimmed = url.trim()
    const error = validateSubscriptionUrl(trimmed)
    if (error) {
      data.showToast(error, 'error')
      return false
    }
    try {
      await data.apiCall('subs', { method: 'POST', body: JSON.stringify({ url: trimmed } satisfies SubRequest) }, 'addSub')
      data.clearDelays()
      const read = await data.fetchSubs()
      showMutationResult('订阅地址已保存，请查看节点获取状态', read, '添加')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const scanClashVerge = async (): Promise<VergeImportResult | null> => {
    try {
      const response = await data.apiCall<VergeImportResult>('import/clash-verge', {}, 'scanVerge')
      return response.data ?? null
    } catch {
      return null
    }
  }

  const importClashVergeSubs = async (urls: string[]): Promise<boolean> => {
    try {
      const response = await data.apiCall<SubBatchResult>(
        'subs/batch',
        { method: 'POST', body: JSON.stringify({ urls } satisfies SubBatchRequest) },
        'importVerge',
      )
      data.clearDelays()
      const read = await data.fetchSubs()
      presentFeedback(vergeImportFeedback(response.data, read))
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const deleteResource = async (
    endpoint: string,
    body: SubRequest | DeleteNodeRequest | DeleteRuleRequest,
    action: string,
    refresh: () => Promise<ResourceReadResult<unknown>>,
    label: string,
  ) => {
    if (deleting.has(action)) return
    deleting.add(action)
    setPendingDeletes(new Set(deleting))
    try {
      await data.apiCall(endpoint, { method: 'DELETE', body: JSON.stringify(body) }, action)
      const result = await refresh()
      if (endpoint !== 'rules') data.clearDelays()
      showMutationResult(`${label}已删除`, result, '删除')
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    } finally {
      deleting.delete(action)
      setPendingDeletes(new Set(deleting))
    }
  }

  const handleDeleteSubscription = (url: string) => {
    void deleteResource('subs', { url }, 'deleteSub', data.fetchSubs, '订阅')
  }

  const handleRefreshSubscriptions = async () => {
    try {
      const response = await data.apiCall<SubscriptionRefreshResult>('subs/refresh', { method: 'POST' }, 'refreshSubs')
      await Promise.all([data.fetchSubs(), data.fetchStatus()])
      data.clearDelays()
      presentFeedback(subscriptionRefreshFeedback(response.data))
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleOpenRefreshSubscriptionsConfirm = () => {
    openConfirm(
      '刷新订阅',
      '确定要刷新所有订阅吗？这将重新获取订阅节点并更新配置。',
      () => { void handleRefreshSubscriptions() },
    )
  }

  const handleSetNodeDisabled = async (sub: string, name: string, disabled: boolean): Promise<boolean> => {
    try {
      await data.apiCall(
        'subs/nodes/disabled',
        { method: 'POST', body: JSON.stringify({ sub, name, disabled } satisfies SetNodeDisabledRequest) },
        'toggleSubNode',
      )
      const read = await data.fetchSubs()
      showMutationResult(disabled ? `已禁用节点 ${name}` : `已启用节点 ${name}`, read, '操作')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const handleAddNode = async (): Promise<boolean> => {
    let payload: NodeRequest
    try {
      payload = buildNodeRequest(data.nodeType(), data.nodeForm())
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }

    try {
      await data.apiCall('nodes', { method: 'POST', body: JSON.stringify(payload) }, 'addNode')
      const read = await data.fetchNodes()
      data.clearDelays()
      showMutationResult('节点已添加', read, '添加')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const handleImportNodes = async (payloads: NodeRequest[]): Promise<BatchNodeResult | undefined> => {
    if (!payloads?.length) return

    try {
      const response = await data.apiCall<BatchNodeResult>(
        'nodes/import',
        { method: 'POST', body: JSON.stringify({ nodes: payloads } satisfies BatchNodeRequest) },
        'importNodes',
      )
      const result = response.data || { added: [], failed: [] }
      if (result.added.length > 0) {
        const read = await data.fetchNodes()
        data.clearDelays()
        showMutationResult(`已添加 ${result.added.length} 个节点`, read, '导入')
      }
      presentFeedback(batchNodeFailuresFeedback(result.failed))
      return result
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return undefined
    }
  }

  const handleDeleteNode = (tag: string) => {
    void deleteResource('nodes', { tag }, 'deleteNode', data.fetchNodes, '节点')
  }

  const handleTestDelay = (nodeName: string) => {
    void data.testDelay(data.clashApiBase, nodeName)
  }

  const handleTestGroupDelays = (groupName: string, nodeNames: string[]) => {
    void data.testGroupDelays(data.clashApiBase, groupName, nodeNames)
  }

  const handleOpenConnections = () => {
    if (window.matchMedia(`(max-width: ${CONNECTIONS_MODAL_MIN_WIDTH - 1}px)`).matches) {
      data.showToast('移动端暂不支持链接统计面板', 'info')
      return
    }
    data.setShowConnectionsModal(true)
  }

  const handleStartService = async () => {
    try {
      await data.apiCall('service/start', { method: 'POST' }, 'startService')
      await data.fetchStatus()
      await data.fetchProxies()
      data.showToast('代理服务已启动', 'success')
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleStopService = async () => {
    try {
      await data.apiCall('service/stop', { method: 'POST' }, 'stopService')
      await data.fetchStatus()
      data.showToast('代理服务已停止', 'info')
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleOpenStopServiceConfirm = () => {
    openConfirm(
      '停止代理服务',
      '停止后透明分流与独立代理端口将暂停转发，确定要停止服务吗？',
      () => { void handleStopService() },
    )
  }

  const handleUpsertTerminal = async (req: UpsertTerminalRequest): Promise<boolean> => {
    try {
      await data.apiCall<TerminalSummary>(
        'terminals',
        { method: 'POST', body: JSON.stringify(req) },
        'upsertTerminal',
      )
      const read = await data.fetchTerminals()
      showMutationResult(`已保存设备备注「${req.name.trim()}」`, read, '保存')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const handleDeleteTerminal = async (ip: string): Promise<boolean> => {
    try {
      await data.apiCall(
        'terminals',
        { method: 'DELETE', body: JSON.stringify({ ip } satisfies DeleteTerminalRequest) },
        'deleteTerminal',
      )
      const read = await data.fetchTerminals()
      showMutationResult('已清除设备备注', read, '清除')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const handleAssignTerminal = async (req: AssignTerminalRequest): Promise<boolean> => {
    try {
      await data.apiCall(
        'terminals/assign',
        { method: 'POST', body: JSON.stringify(req) },
        `assignTerminal:${req.ip}`,
      )
      const [rulesRead] = await Promise.all([data.fetchRules(), data.fetchTerminals(), data.fetchStatus()])
      const label = req.mode === 'dedicated'
        ? `设备 ${req.ip} 已绑定节点「${req.target}」`
        : req.mode === 'block_overseas'
          ? `设备 ${req.ip} 已设为禁止走代理`
          : req.mode === 'direct'
            ? `设备 ${req.ip} 已设为全量直连`
            : `设备 ${req.ip} 已恢复使用默认代理`
      showMutationResult(label, rulesRead, '切换')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const handleSaveAccessPort = async (req: SaveAccessPortRequest): Promise<boolean> => {
    try {
      await data.apiCall<AccessPortStatus>(
        'access-ports',
        { method: 'POST', body: JSON.stringify(req) },
        'saveAccessPort',
      )
      const [portsRead] = await Promise.all([data.fetchAccessPorts(), data.fetchStatus()])
      showMutationResult(`独立端口「${req.name.trim()}」(:${req.port}) 已保存`, portsRead, '保存')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const handleDeleteAccessPort = async (id: string, name: string) => {
    try {
      await data.apiCall(
        'access-ports',
        { method: 'DELETE', body: JSON.stringify({ id } satisfies DeleteAccessPortRequest) },
        `deleteAccessPort:${id}`,
      )
      const [portsRead] = await Promise.all([data.fetchAccessPorts(), data.fetchStatus()])
      showMutationResult(`独立端口「${name}」已删除`, portsRead, '删除')
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleOpenDeleteAccessPortConfirm = (port: AccessPortStatus) => {
    openConfirm(
      '删除独立代理端口',
      `确定要关闭并删除「${port.name}」(端口 :${port.port}) 吗？使用该端口的客户端将无法连接。`,
      () => { void handleDeleteAccessPort(port.id, port.name) },
    )
  }

  const runConfirmedUpgrade = async (currentVersion: string, targetVersion: string) => {
    data.setUpgrading(true)
    setUpgradeMessage(`正在请求升级 ${currentVersion} → ${targetVersion}；后端可能需要下载、校验和安装，请勿关闭或刷新页面。`)
    try {
      let installedVersion = targetVersion
      try {
        const response = await data.apiCall<string>('upgrade', { method: 'POST' })
        if (!response.data) {
          await data.fetchVersion()
          data.showToast('后端已是最新版本，无需升级', 'info')
          return
        }
        installedVersion = response.data
        setUpgradeMessage(`后端已完成安装，正在确认 ${installedVersion} 启动；短暂断连属于正常现象，请勿重复升级。`)
      } catch (error) {
        if (!isUncertainRequestError(error)) throw error
        setUpgradeMessage(`升级请求连接中断，结果未知；正在核对 ${installedVersion} 是否启动，请勿重复升级。`)
      }
      await waitForUpgrade(installedVersion, currentVersion)
      window.location.reload()
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    } finally {
      data.setUpgrading(false)
    }
  }

  const handleUpgradeClick = async () => {
    const info = data.versionInfo()
    if (info.upgrade_supported === false) {
      data.showToast('当前平台请下载安装包，退出后覆盖安装', 'info')
      return
    }

    if (!info.has_update) {
      const read = await data.fetchVersion()
      const fresh = read.status === 'success' ? read.data : undefined
      presentFeedback(describeVersionCheckResult(fresh))
      return
    }

    const targetVersion = info.latest
    if (!targetVersion) return
    const currentVersion = info.current
    openConfirm(
      '更新确认',
      `确定要从 ${currentVersion} 更新到 ${targetVersion} 吗？更新过程中服务会短暂中断。`,
      () => { void runConfirmedUpgrade(currentVersion, targetVersion) },
    )
  }

  const handleOpenDeleteNodeConfirm = (tag: string) => {
    openConfirm('删除节点', `确定要删除节点 "${tag}" 吗？`, () => handleDeleteNode(tag))
  }

  const handleOpenDeleteSubConfirm = (url: string) => {
    openConfirm('删除订阅', `确定要删除此订阅吗？\n${url}`, () => handleDeleteSubscription(url))
  }

  const handleAddRule = async ({ field, value, target }: RuleRequest): Promise<boolean> => {
    try {
      await data.apiCall('rules', { method: 'POST', body: JSON.stringify({ field, value, target } satisfies RuleRequest) }, 'addRule')
      const read = await data.fetchRules()
      showMutationResult('规则已添加', read, '添加')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const handleDeleteRule = (rule: RuleInfo) => {
    void deleteResource('rules', { index: rule.index, raw: rule.raw }, 'deleteRule', data.fetchRules, '规则')
  }

  const handleOpenDeleteRuleConfirm = (rule: RuleInfo) => {
    const label = rule?.field && rule?.value ? `${rule.field}: ${rule.value}` : (rule?.raw || '')
    openConfirm('删除规则', `确定要删除此规则吗？\n${label}`, () => handleDeleteRule(rule))
  }

  const handleToggleMcp = async (enabled: boolean) => {
    try {
      await data.apiCall('mcp', { method: 'POST', body: JSON.stringify({ enabled } satisfies McpRequest) }, 'toggleMcp')
      await data.fetchStatus()
      data.showToast(enabled ? 'MCP 端点已开启' : 'MCP 端点已关闭', 'success')
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
    }
  }

  const handleSaveScheduledRefresh = async (request: ScheduledRefreshRequest) => {
    try {
      await data.apiCall('scheduled-refresh', {
        method: 'POST',
        body: JSON.stringify(request),
      }, 'scheduledRefresh')
      data.showToast(request.enabled ? '定时刷新已启用' : '定时刷新已关闭', 'success')
      return true
    } catch (error) {
      data.showToast(errorMessage(error), 'error')
      return false
    }
  }

  const pendingActions = createMemo<ReadonlySet<string>>(
    () => new Set([...data.pendingActions(), ...pendingDeletes()]),
  )

  return {
    pendingActions,
    openConfirm,
    closeConfirm,
    openNodeModal,
    closeNodeModal,
    handleOpenSetRouteModeConfirm,
    handleSetMaxMultiplier,
    handleSetNodeSelect,
    handleSwitchProxy,
    handleAddSubscription,
    handleOnboardingAddSub: handleAddSubscription,
    scanClashVerge,
    importClashVergeSubs,
    handleRefreshSubscriptions,
    handleOpenRefreshSubscriptionsConfirm,
    handleAddNode,
    handleImportNodes,
    handleDeployVps,
    vpsTasks,
    vpsDeploying,
    handleTestDelay,
    handleTestGroupDelays,
    handleOpenConnections,
    handleStartService,
    handleStopService,
    handleOpenStopServiceConfirm,
    handleUpsertTerminal,
    handleDeleteTerminal,
    handleAssignTerminal,
    handleSaveAccessPort,
    handleOpenDeleteAccessPortConfirm,
    handleUpgradeClick,
    upgradeMessage,
    handleOpenDeleteNodeConfirm,
    handleOpenDeleteSubConfirm,
    handleSetNodeDisabled,
    handleAddRule,
    handleOpenDeleteRuleConfirm,
    handleToggleMcp,
    handleSaveScheduledRefresh,
  }
}
