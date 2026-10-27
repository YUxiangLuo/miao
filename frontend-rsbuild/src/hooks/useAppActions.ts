import { useCallback, useRef, useState } from 'react'
import { CONNECTIONS_MODAL_MIN_WIDTH } from '../tokens'
import { validateSubscriptionUrl } from '../utils'
import { buildNodeRequest } from '../nodeForm'
import { waitForUpgrade } from './upgrade'
import { RequestInterruptedError } from './request'
import { useVpsDeployment } from './useVpsDeployment'
import { mutationFeedback } from './mutationFeedback'
import type { ResourceReadResult } from './useResources'
import type { ToastTone } from './useApi'
import type { useAppData } from './useAppData'
import type {
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
  SwitchProxyResult,
  BatchNodeResult,
  NodeSelect,
  RouteMode,
  RuleInfo,
  RuleRequest,
  ScheduledRefreshRequest,
  SubBatchResult,
  VergeImportResult,
  SubscriptionRefreshResult,
} from '../types/api'

type AppData = ReturnType<typeof useAppData>

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export function useAppActions(data: AppData) {
  const { vpsTasks, vpsDeploying, handleDeployVps } = useVpsDeployment(data)
  const [upgradeMessage, setUpgradeMessage] = useState('')
  const deleting = useRef(new Set<string>())
  const [pendingDeletes, setPendingDeletes] = useState<ReadonlySet<string>>(new Set())
  const {
    status,
    apiCall,
    showToast,
    clearDelays,
    fetchStatus,
    fetchProxies,
    fetchSubs,
    fetchNodes,
    fetchRules,
    fetchVersion,
    versionInfo,
    clashApiBase,
    switchingNode,
    setSwitchingNode,
    nodeType,
    nodeForm,
    setShowNodeModal,
    setShowConnectionsModal,
    setConfirmState,
    setUpgrading,
    resetNodeForm,
    testDelay,
    testGroupDelays,
  } = data
  const requestedNodeSelect = status.requested_node_select || status.node_select || 'manual'

  const showMutationResult = useCallback((message: string, read: ResourceReadResult<unknown>, action: string, tone?: ToastTone) => {
    const feedback = mutationFeedback(message, read, action, tone)
    showToast(feedback.message, feedback.tone)
  }, [showToast])

  const openConfirm = useCallback((title: string, message: string, onConfirm: () => void) => {
    setConfirmState({ open: true, title, message, onConfirm })
  }, [setConfirmState])

  const closeConfirm = useCallback(() => {
    setConfirmState({ open: false, title: '', message: '', onConfirm: null })
  }, [setConfirmState])

  const openNodeModal = useCallback(() => {
    if (status.initializing) {
      showToast('初始化完成后才能修改节点', 'info')
      return
    }
    setShowNodeModal(true)
  }, [status.initializing, showToast, setShowNodeModal])

  const closeNodeModal = useCallback(() => {
    setShowNodeModal(false)
    resetNodeForm()
  }, [resetNodeForm, setShowNodeModal])

  const handleSetRouteMode = useCallback(async (nextMode: RouteMode) => {
    if (nextMode === status.route_mode) return

    try {
      await apiCall<RouteModeResult>(
        'route-mode',
        { method: 'POST', body: JSON.stringify({ route_mode: nextMode } satisfies RouteModeRequest) },
        'routeMode'
      )
      clearDelays()
      await fetchStatus()
      await fetchProxies()
      showToast(nextMode === 'global' ? '已切换为全局代理' : '已切换为分流模式', 'success')
    } catch (error) {
      showToast(errorMessage(error), 'error')
    }
  }, [
    status.route_mode,
    apiCall,
    clearDelays,
    fetchStatus,
    fetchProxies,
    showToast
  ])

  const handleOpenSetRouteModeConfirm = useCallback((nextMode: RouteMode) => {
    if (nextMode === status.route_mode) return
    if (nextMode === 'global') {
      openConfirm(
        '切换为全局代理',
        '关闭内网和国内直连分流，按自定义规则选择出口，其余流量走默认代理（含国内站点）。切换时服务会短暂中断。',
        () => handleSetRouteMode('global')
      )
      return
    }
    openConfirm(
      '切换为分流模式',
      '内网和中国大陆先直连，再匹配自定义规则；其余流量使用默认代理。切换时服务会短暂中断。',
      () => handleSetRouteMode('rule')
    )
  }, [status.route_mode, openConfirm, handleSetRouteMode])

  const handleSetMaxMultiplier = useCallback(async (nextMultiplier: string | null) => {
    if (nextMultiplier === (status.max_multiplier ?? null)) return

    try {
      await apiCall<MaxMultiplierResult>(
        'max-multiplier',
        { method: 'POST', body: JSON.stringify({ max_multiplier: nextMultiplier } satisfies MaxMultiplierRequest) },
        'maxMultiplier'
      )
      clearDelays()
      await fetchStatus()
      await fetchProxies()
      showToast(nextMultiplier === null ? '已取消倍率限制' : `最高倍率已设为 ${nextMultiplier}x`, 'success')
    } catch (error) {
      showToast(errorMessage(error), 'error')
    }
  }, [
    status.max_multiplier,
    apiCall,
    clearDelays,
    fetchStatus,
    fetchProxies,
    showToast
  ])

  const handleSetNodeSelect = useCallback(async (nextSelect: NodeSelect) => {
    if (nextSelect === requestedNodeSelect) return

    try {
      const payload = await apiCall<NodeSelectResult>(
        'node-select',
        { method: 'POST', body: JSON.stringify({ node_select: nextSelect } satisfies NodeSelectRequest) },
        'nodeSelect'
      )
      clearDelays()
      const read = await fetchStatus()
      await fetchProxies()
      if (!payload.data) {
        showToast('节点选择请求已处理，请查看当前策略', 'info')
      } else if (payload.data.requested !== 'manual' && payload.data.node_select === 'manual') {
        // The status hook already presents its warning. Use the command result
        // when status could not be read or did not provide a warning.
        if (read.status !== 'success' || !read.data.warning) {
          showToast('请求的地区暂无可用节点，当前使用手动选择', 'info')
        }
      } else {
        showToast('节点选择已更新', 'success')
      }
    } catch (error) {
      showToast(errorMessage(error), 'error')
    }
  }, [
    requestedNodeSelect,
    apiCall,
    clearDelays,
    fetchStatus,
    fetchProxies,
    showToast
  ])

  const handleSwitchProxy = useCallback(async (groupName: string, nodeName: string) => {
    if (requestedNodeSelect !== 'manual') return
    if (switchingNode) return
    setSwitchingNode(nodeName)
    try {
      const response = await apiCall<SwitchProxyResult>('proxy/switch', {
        method: 'POST', body: JSON.stringify({ group: groupName, name: nodeName } satisfies LastProxy),
      }, 'switchProxy')
      await fetchProxies()
      showToast(response.data?.persisted
        ? `已切换到 ${nodeName}`
        : `已切换到 ${nodeName}，但保存选择失败，重启后可能恢复旧节点`,
      response.data?.persisted ? 'success' : 'error')
    } catch (error) {
      showToast(errorMessage(error) || '切换节点失败', 'error')
    } finally {
      setSwitchingNode('')
    }
  }, [requestedNodeSelect, apiCall, fetchProxies, showToast, switchingNode, setSwitchingNode])

  const handleAddSubscription = useCallback(async (url: string): Promise<boolean> => {
    const trimmed = url.trim()
    const error = validateSubscriptionUrl(trimmed)
    if (error) {
      showToast(error, 'error')
      return false
    }
    try {
      await apiCall('subs', { method: 'POST', body: JSON.stringify({ url: trimmed } satisfies SubRequest) }, 'addSub')
      clearDelays()
      const read = await fetchSubs()
      showMutationResult('订阅地址已保存，请查看节点获取状态', read, '添加')
      return true
    } catch (error) {
      showToast(errorMessage(error), 'error')
      return false
    }
  }, [apiCall, clearDelays, fetchSubs, showToast, showMutationResult])

  // 扫描本机 clash-verge-rev 的订阅（只读）；失败/未安装都返回 null，由调用方提示
  const scanClashVerge = useCallback(async (): Promise<VergeImportResult | null> => {
    try {
      const response = await apiCall<VergeImportResult>('import/clash-verge', {}, 'scanVerge')
      return response.data ?? null
    } catch {
      return null
    }
  }, [apiCall])

  const importClashVergeSubs = useCallback(async (urls: string[]): Promise<boolean> => {
    try {
      const response = await apiCall<SubBatchResult>(
        'subs/batch',
        { method: 'POST', body: JSON.stringify({ urls } satisfies SubBatchRequest) },
        'importVerge'
      )
      clearDelays()
      const read = await fetchSubs()
      showMutationResult(response.data
        ? `已新增 ${response.data.added} 条订阅地址，跳过 ${response.data.skipped} 条已有订阅。请查看节点获取状态。`
        : '订阅导入请求已完成，请查看订阅列表和节点获取状态。', read, '导入', response.data?.added ? 'success' : 'info')
      return true
    } catch (error) {
      showToast(errorMessage(error), 'error')
      return false
    }
  }, [apiCall, clearDelays, fetchSubs, showToast, showMutationResult])

  const deleteResource = useCallback(async (endpoint: string, body: SubRequest | DeleteNodeRequest | DeleteRuleRequest, action: string, refresh: () => Promise<ResourceReadResult<unknown>>, label: string) => {
    if (deleting.current.has(action)) return
    deleting.current.add(action)
    setPendingDeletes(new Set(deleting.current))
    try {
      await apiCall(endpoint, { method: 'DELETE', body: JSON.stringify(body) }, action)
      const result = await refresh()
      if (endpoint !== 'rules') clearDelays()
      showMutationResult(`${label}已删除`, result, '删除')
    } catch (error) {
      showToast(errorMessage(error), 'error')
    } finally {
      deleting.current.delete(action)
      setPendingDeletes(new Set(deleting.current))
    }
  }, [apiCall, clearDelays, showToast, showMutationResult])

  const handleDeleteSubscription = useCallback((url: string) => deleteResource('subs', { url }, 'deleteSub', fetchSubs, '订阅'), [deleteResource, fetchSubs])

  const handleRefreshSubscriptions = useCallback(async () => {
    try {
      const response = await apiCall<SubscriptionRefreshResult>('subs/refresh', { method: 'POST' }, 'refreshSubs')
      await Promise.all([fetchSubs(), fetchStatus()])
      clearDelays()
      const result = response.data
      if (!result) {
        showToast('刷新请求已处理；后端未提供详细结果，请查看订阅状态。', 'info')
      } else if (!result.fetch_succeeded) {
        showToast('订阅获取失败，已保留原配置。请查看订阅错误详情。', 'error')
      } else if (result.report?.failed_sources > 0) {
        showToast(`订阅部分获取成功：成功 ${result.report.successful_sources} 个，失败 ${result.report.failed_sources} 个。请查看订阅详情。${result.warning || ''}`, 'error')
      } else if (result.warning) {
        showToast(`订阅刷新有告警：${result.warning}`, 'error')
      } else {
        showToast(result.runtime_updated ? '订阅已获取，代理配置已更新' : '订阅已获取，代理运行状态未改变', 'success')
      }
    } catch (error) {
      showToast(errorMessage(error), 'error')
    }
  }, [apiCall, clearDelays, fetchSubs, fetchStatus, showToast])

  const handleOpenRefreshSubscriptionsConfirm = useCallback(() => {
    openConfirm(
      '刷新订阅',
      '确定要刷新所有订阅吗？这将重新获取订阅节点并更新配置。',
      () => handleRefreshSubscriptions()
    )
  }, [openConfirm, handleRefreshSubscriptions])

  // 禁用/启用订阅节点（易变层变更，后端热应用）；成功后刷新订阅列表更新禁用计数
  const handleSetNodeDisabled = useCallback(async (sub: string, name: string, disabled: boolean): Promise<boolean> => {
    try {
      await apiCall(
        'subs/nodes/disabled',
        { method: 'POST', body: JSON.stringify({ sub, name, disabled } satisfies SetNodeDisabledRequest) },
        'toggleSubNode'
      )
      const read = await fetchSubs()
      showMutationResult(disabled ? `已禁用节点 ${name}` : `已启用节点 ${name}`, read, '操作')
      return true
    } catch (error) {
      showToast(errorMessage(error), 'error')
      return false
    }
  }, [apiCall, fetchSubs, showToast, showMutationResult])

  const handleAddNode = useCallback(async () => {
    let payload: NodeRequest
    try {
      payload = buildNodeRequest(nodeType, nodeForm)
    } catch (error) {
      showToast(errorMessage(error), 'error')
      return false
    }

    try {
      await apiCall('nodes', { method: 'POST', body: JSON.stringify(payload) }, 'addNode')
      const read = await fetchNodes()
      clearDelays()
      showMutationResult('节点已添加', read, '添加')
      return true
    } catch (error) {
      showToast(errorMessage(error), 'error')
      return false
    }
  }, [nodeForm, nodeType, apiCall, clearDelays, fetchNodes, showToast, showMutationResult])

  const handleImportNodes = useCallback(async (payloads: NodeRequest[]) => {
    if (!payloads?.length) return

    try {
      const response = await apiCall<BatchNodeResult>(
        'nodes/import',
        { method: 'POST', body: JSON.stringify({ nodes: payloads } satisfies BatchNodeRequest) },
        'importNodes',
      )
      const result = response.data || { added: [], failed: [] }
      if (result.added.length > 0) {
        const read = await fetchNodes()
        clearDelays()
        showMutationResult(`已添加 ${result.added.length} 个节点`, read, '导入')
      }
      if (result.failed.length > 0) {
        const failures = result.failed.map((item) => `${item.tag}: ${item.message}`)
        const shown = failures.slice(0, 2).join('; ')
        const more = failures.length > 2 ? ` 等 ${failures.length} 项` : ''
        showToast(`导入失败: ${shown}${more}`, 'error')
      }
      return result
    } catch (error) {
      showToast(errorMessage(error), 'error')
    }
  }, [apiCall, clearDelays, fetchNodes, showToast, showMutationResult])

  const handleDeleteNode = useCallback((tag: string) => deleteResource('nodes', { tag }, 'deleteNode', fetchNodes, '节点'), [deleteResource, fetchNodes])

  const handleTestDelay = useCallback((nodeName: string) => {
    testDelay(clashApiBase, nodeName)
  }, [clashApiBase, testDelay])

  const handleTestGroupDelays = useCallback((groupName: string, nodeNames: string[]) => {
    testGroupDelays(clashApiBase, groupName, nodeNames)
  }, [clashApiBase, testGroupDelays])

  const handleOpenConnections = useCallback(() => {
    if (window.matchMedia(`(max-width: ${CONNECTIONS_MODAL_MIN_WIDTH - 1}px)`).matches) {
      showToast('移动端暂不支持链接统计面板', 'info')
      return
    }

    setShowConnectionsModal(true)
  }, [showToast, setShowConnectionsModal])

  const handleStartService = useCallback(async () => {
    try {
      await apiCall('service/start', { method: 'POST' }, 'startService')
      await fetchStatus()
      await fetchProxies()
      showToast('代理服务已重新启动', 'success')
    } catch (error) {
      showToast(errorMessage(error), 'error')
    }
  }, [apiCall, fetchProxies, fetchStatus, showToast])

  const handleUpgradeClick = useCallback(async () => {
    if (versionInfo.upgrade_supported === false) {
      showToast('当前平台请下载安装包，退出后覆盖安装', 'info')
      return
    }

    if (!versionInfo.has_update) {
      const read = await fetchVersion()
      const fresh = read.status === 'success' ? read.data : undefined
      if (fresh?.has_update) {
        showToast(`发现新版本 ${fresh.latest}`, 'success')
      } else if (!fresh?.latest) {
        showToast('暂未获取到最新版本信息，请稍后重试', 'info')
      } else {
        showToast('当前已是最新版本', 'info')
      }
      return
    }

    const targetVersion = versionInfo.latest
    if (!targetVersion) return
    const currentVersion = versionInfo.current
    openConfirm('更新确认', `确定要从 ${currentVersion} 更新到 ${targetVersion} 吗？更新过程中服务会短暂中断。`, async () => {
      setUpgrading(true)
      setUpgradeMessage(`正在请求升级 ${currentVersion} → ${targetVersion}；后端可能需要下载、校验和安装，请勿关闭或刷新页面。`)
      try {
        let installedVersion = targetVersion
        try {
          const response = await apiCall<string>('upgrade', { method: 'POST' })
          if (!response.data) {
            await fetchVersion()
            showToast('后端已是最新版本，无需升级', 'info')
            return
          }
          installedVersion = response.data
          setUpgradeMessage(`后端已完成安装，正在确认 ${installedVersion} 启动；短暂断连属于正常现象，请勿重复升级。`)
        } catch (error) {
          if (!(error instanceof RequestInterruptedError || error instanceof TypeError)) throw error
          setUpgradeMessage(`升级请求连接中断，结果未知；正在核对 ${installedVersion} 是否启动，请勿重复升级。`)
        }
        await waitForUpgrade(installedVersion, currentVersion)
        window.location.reload()
      } catch (error) {
        showToast(errorMessage(error), 'error')
      } finally {
        setUpgrading(false)
      }
    })
  }, [versionInfo, fetchVersion, showToast, openConfirm, setUpgrading, apiCall])

  const handleOpenDeleteNodeConfirm = useCallback((tag: string) => {
    openConfirm('删除节点', `确定要删除节点 "${tag}" 吗？`, () => handleDeleteNode(tag))
  }, [openConfirm, handleDeleteNode])

  const handleOpenDeleteSubConfirm = useCallback((url: string) => {
    openConfirm('删除订阅', `确定要删除此订阅吗？\n${url}`, () => handleDeleteSubscription(url))
  }, [openConfirm, handleDeleteSubscription])

  const handleAddRule = useCallback(async ({ field, value, target }: RuleRequest): Promise<boolean> => {
    try {
      await apiCall('rules', { method: 'POST', body: JSON.stringify({ field, value, target } satisfies RuleRequest) }, 'addRule')
      const read = await fetchRules()
      showMutationResult('规则已添加', read, '添加')
      return true
    } catch (error) {
      showToast(errorMessage(error), 'error')
      return false
    }
  }, [apiCall, fetchRules, showToast, showMutationResult])

  const handleDeleteRule = useCallback((rule: RuleInfo) => deleteResource('rules', { index: rule.index, raw: rule.raw }, 'deleteRule', fetchRules, '规则'), [deleteResource, fetchRules])

  const handleOpenDeleteRuleConfirm = useCallback((rule: RuleInfo) => {
    const label = rule?.field && rule?.value ? `${rule.field}: ${rule.value}` : (rule?.raw || '')
    openConfirm('删除规则', `确定要删除此规则吗？\n${label}`, () => handleDeleteRule(rule))
  }, [openConfirm, handleDeleteRule])

  const handleToggleMcp = useCallback(async (enabled: boolean) => {
    try {
      await apiCall('mcp', { method: 'POST', body: JSON.stringify({ enabled } satisfies McpRequest) }, 'toggleMcp')
      await fetchStatus()
      showToast(enabled ? 'MCP 端点已开启' : 'MCP 端点已关闭', 'success')
    } catch (error) {
      showToast(errorMessage(error), 'error')
    }
  }, [apiCall, fetchStatus, showToast])

  const handleSaveScheduledRefresh = useCallback(async (request: ScheduledRefreshRequest) => {
    try {
      await apiCall('scheduled-refresh', {
        method: 'POST',
        body: JSON.stringify(request),
      }, 'scheduledRefresh')
      showToast(request.enabled ? '定时刷新已启用' : '定时刷新已关闭', 'success')
      return true
    } catch (error) {
      showToast(errorMessage(error), 'error')
      return false
    }
  }, [apiCall, showToast])

  return {
    pendingActions: new Set([...data.pendingActions, ...pendingDeletes]),
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
