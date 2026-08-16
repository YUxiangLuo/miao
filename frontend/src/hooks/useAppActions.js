import { useCallback } from 'react'
import { CONNECTIONS_MODAL_MIN_WIDTH } from '../layout.js'
import { validateSubscriptionUrl } from '../utils.js'
import { buildNodeRequest } from '../nodeForm.js'

export function useAppActions(data) {
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
    fetchConnections,
    fetchVersion,
    versionInfo,
    clashApiBase,
    switchingNode,
    setSwitchingNode,
    newSubUrl,
    setNewSubUrl,
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

  const openConfirm = useCallback((title, message, onConfirm) => {
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

  const handleToggleService = useCallback(async () => {
    try {
      if (status.running) {
        await apiCall('service/stop', { method: 'POST' }, 'stop')
        clearDelays()
        showToast('服务已停止', 'success')
      } else {
        await apiCall('service/start', { method: 'POST' }, 'start')
        showToast('服务已启动', 'success')
      }
      await fetchStatus()
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [status.running, apiCall, clearDelays, fetchStatus, showToast])

  const handleSetRouteMode = useCallback(async (nextMode) => {
    if (nextMode === status.route_mode) return

    try {
      await apiCall(
        'route-mode',
        { method: 'POST', body: JSON.stringify({ route_mode: nextMode }) },
        'routeMode'
      )
      clearDelays()
      await fetchStatus()
      await fetchProxies()
      showToast(nextMode === 'global' ? '已切换为全局代理' : '已切换为分流模式', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [
    status.route_mode,
    apiCall,
    clearDelays,
    fetchStatus,
    fetchProxies,
    showToast
  ])

  const handleOpenSetRouteModeConfirm = useCallback((nextMode) => {
    if (nextMode === status.route_mode) return
    if (nextMode === 'global') {
      openConfirm(
        '切换为全局代理',
        '确定要切换为全局代理吗？所有流量（含国内站点）都将走代理，切换时服务会短暂中断。',
        () => handleSetRouteMode('global')
      )
      return
    }
    openConfirm(
      '切换为分流模式',
      '确定要切换为分流模式吗？国内流量将直连，国外流量走代理，切换时服务会短暂中断。',
      () => handleSetRouteMode('rule')
    )
  }, [status.route_mode, openConfirm, handleSetRouteMode])

  const handleSwitchProxy = useCallback(async (groupName, nodeName) => {
    if (switchingNode) return
    setSwitchingNode(nodeName)
    try {
      const response = await fetch(`${clashApiBase}/proxies/${encodeURIComponent(groupName)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: nodeName }),
      })
      if (!response.ok) {
        const details = (await response.text()).trim()
        throw new Error(details || `切换节点失败 (${response.status})`)
      }
      await fetchProxies()
      fetch('/api/last-proxy', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ group: groupName, name: nodeName }),
      }).catch((err) => console.warn('Failed to save last proxy:', err))
      showToast(`已切换到 ${nodeName}`, 'success')
    } catch (error) {
      showToast(error.message || '切换节点失败', 'error')
    } finally {
      setSwitchingNode('')
    }
  }, [clashApiBase, fetchProxies, showToast, switchingNode, setSwitchingNode])

  const handleAddSubscription = useCallback(async () => {
    const error = validateSubscriptionUrl(newSubUrl.trim())
    if (error) {
      showToast(error, 'error')
      return
    }
    try {
      await apiCall('subs', { method: 'POST', body: JSON.stringify({ url: newSubUrl.trim() }) }, 'addSub')
      setNewSubUrl('')
      clearDelays()
      await fetchSubs()
      showToast('订阅已添加', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [newSubUrl, apiCall, clearDelays, fetchSubs, showToast, setNewSubUrl])

  const handleOnboardingAddSub = useCallback(async (url) => {
    try {
      await apiCall('subs', { method: 'POST', body: JSON.stringify({ url }) }, 'addSub')
      clearDelays()
      await fetchSubs()
      showToast('订阅已添加', 'success')
      return true
    } catch (error) {
      showToast(error.message, 'error')
      return false
    }
  }, [apiCall, clearDelays, fetchSubs, showToast])

  const handleDeleteSubscription = useCallback(async (url) => {
    try {
      await apiCall('subs', { method: 'DELETE', body: JSON.stringify({ url }) }, 'deleteSub')
      await fetchSubs()
      clearDelays()
      showToast('订阅已删除', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [apiCall, clearDelays, fetchSubs, showToast])

  const handleRefreshSubscriptions = useCallback(async () => {
    try {
      await apiCall('subs/refresh', { method: 'POST' }, 'refreshSubs')
      await fetchSubs()
      clearDelays()
      showToast('订阅已刷新', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [apiCall, clearDelays, fetchSubs, showToast])

  const handleAddNode = useCallback(async () => {
    let payload
    try {
      payload = buildNodeRequest(nodeType, nodeForm)
    } catch (error) {
      showToast(error.message, 'error')
      return
    }

    try {
      await apiCall('nodes', { method: 'POST', body: JSON.stringify(payload) }, 'addNode')
      closeNodeModal()
      await fetchNodes()
      clearDelays()
      showToast('节点已添加', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [nodeForm, nodeType, apiCall, clearDelays, closeNodeModal, fetchNodes, showToast])

  const handleImportNodes = useCallback(async (payloads) => {
    if (!payloads?.length) return

    const failures = []
    let added = 0
    for (const payload of payloads) {
      try {
        await apiCall('nodes', { method: 'POST', body: JSON.stringify(payload) })
        added += 1
      } catch (error) {
        failures.push(`${payload.tag}: ${error.message}`)
      }
    }

    if (added > 0) {
      await fetchNodes()
      clearDelays()
      showToast(`已添加 ${added} 个节点`, 'success')
    }
    if (failures.length > 0) {
      const shown = failures.slice(0, 2).join('; ')
      const more = failures.length > 2 ? ` 等 ${failures.length} 项` : ''
      showToast(`导入失败: ${shown}${more}`, 'error')
    } else {
      closeNodeModal()
    }
  }, [apiCall, clearDelays, closeNodeModal, fetchNodes, showToast])

  const handleDeployVps = useCallback(async ({ ip, password }) => {
    try {
      const payload = await apiCall(
        'vps/deploy',
        { method: 'POST', body: JSON.stringify({ ip, password }) },
        'deployVps',
      )
      closeNodeModal()
      await fetchNodes()
      clearDelays()
      showToast(payload.message, 'success')
      return true
    } catch (error) {
      showToast(error.message, 'error')
      return false
    }
  }, [apiCall, clearDelays, closeNodeModal, fetchNodes, showToast])

  const handleDeleteNode = useCallback(async (tag) => {
    try {
      await apiCall('nodes', { method: 'DELETE', body: JSON.stringify({ tag }) }, 'deleteNode')
      await fetchNodes()
      clearDelays()
      showToast('节点已删除', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [apiCall, clearDelays, fetchNodes, showToast])

  const handleTestDelay = useCallback((nodeName) => {
    testDelay(clashApiBase, nodeName)
  }, [clashApiBase, testDelay])

  const handleTestGroupDelays = useCallback((groupName, nodeNames) => {
    testGroupDelays(clashApiBase, groupName, nodeNames)
  }, [clashApiBase, testGroupDelays])

  const handleOpenConnections = useCallback(() => {
    if (window.matchMedia(`(max-width: ${CONNECTIONS_MODAL_MIN_WIDTH - 1}px)`).matches) {
      showToast('移动端暂不支持链接统计面板', 'info')
      return
    }

    setShowConnectionsModal(true)
    fetchConnections()
  }, [fetchConnections, showToast, setShowConnectionsModal])

  const handleUpgradeClick = useCallback(async () => {
    if (versionInfo.upgrade_supported === false) {
      showToast('当前平台请下载安装包，退出后覆盖安装', 'info')
      return
    }

    if (!status.running) {
      showToast('sing-box 未运行，暂不检测更新', 'info')
      return
    }

    if (!versionInfo.has_update) {
      const fresh = await fetchVersion()
      if (fresh?.has_update) {
        showToast(`发现新版本 ${fresh.latest}`, 'success')
      } else {
        showToast('当前已是最新版本', 'info')
      }
      return
    }

    const targetVersion = versionInfo.latest
    const currentVersion = versionInfo.current
    openConfirm('更新确认', `确定要从 ${currentVersion} 更新到 ${targetVersion} 吗？更新过程中服务会短暂中断。`, async () => {
      setUpgrading(true)
      try {
        const response = await fetch('/api/upgrade', { method: 'POST' })
        const payload = await response.json()
        if (!payload.success) throw new Error(payload.message || '更新失败')
        showToast('更新成功，等待服务重启…', 'success')
        for (let index = 0; index < 30; index += 1) {
          await new Promise((resolve) => window.setTimeout(resolve, 500))
          try {
            const ping = await fetch('/api/version')
            if (ping.ok) {
              const versionPayload = await ping.json()
              if (versionPayload.success && versionPayload.data?.current !== currentVersion) {
                window.location.reload()
                return
              }
            }
          } catch {
            // ignore
          }
        }
        showToast('服务重启超时，请手动刷新页面', 'error')
      } catch (error) {
        showToast(error.message, 'error')
      } finally {
        setUpgrading(false)
      }
    })
  }, [status.running, versionInfo, fetchVersion, showToast, openConfirm, setUpgrading])

  const handleOpenDeleteNodeConfirm = useCallback((tag) => {
    openConfirm('删除节点', `确定要删除节点 "${tag}" 吗？`, () => handleDeleteNode(tag))
  }, [openConfirm, handleDeleteNode])

  const handleOpenDeleteSubConfirm = useCallback((url) => {
    openConfirm('删除订阅', `确定要删除此订阅吗？\n${url}`, () => handleDeleteSubscription(url))
  }, [openConfirm, handleDeleteSubscription])

  const handleAddRule = useCallback(async ({ field, value, target }) => {
    try {
      await apiCall('rules', { method: 'POST', body: JSON.stringify({ field, value, target }) }, 'addRule')
      await fetchRules()
      showToast('规则已添加', 'success')
      return true
    } catch (error) {
      showToast(error.message, 'error')
      return false
    }
  }, [apiCall, fetchRules, showToast])

  const handleDeleteRule = useCallback(async (rule) => {
    try {
      await apiCall('rules', { method: 'DELETE', body: JSON.stringify({ index: rule.index, raw: rule.raw }) }, 'deleteRule')
      await fetchRules()
      showToast('规则已删除', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [apiCall, fetchRules, showToast])

  const handleOpenDeleteRuleConfirm = useCallback((rule) => {
    const label = rule?.field && rule?.value ? `${rule.field}: ${rule.value}` : (rule?.raw || '')
    openConfirm('删除规则', `确定要删除此规则吗？\n${label}`, () => handleDeleteRule(rule))
  }, [openConfirm, handleDeleteRule])

  const handleToggleAdblock = useCallback(async (enabled) => {
    try {
      await apiCall('adblock', { method: 'POST', body: JSON.stringify({ enabled }) }, 'toggleAdblock')
      await fetchStatus()
      showToast(enabled ? '去广告已开启' : '去广告已关闭', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [apiCall, fetchStatus, showToast])

  const handleToggleMcp = useCallback(async (enabled) => {
    try {
      await apiCall('mcp', { method: 'POST', body: JSON.stringify({ enabled }) }, 'toggleMcp')
      await fetchStatus()
      showToast(enabled ? 'MCP 端点已开启' : 'MCP 端点已关闭', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [apiCall, fetchStatus, showToast])

  return {
    openConfirm,
    closeConfirm,
    openNodeModal,
    closeNodeModal,
    handleToggleService,
    handleOpenSetRouteModeConfirm,
    handleSwitchProxy,
    handleAddSubscription,
    handleOnboardingAddSub,
    handleRefreshSubscriptions,
    handleAddNode,
    handleImportNodes,
    handleDeployVps,
    handleTestDelay,
    handleTestGroupDelays,
    handleOpenConnections,
    handleUpgradeClick,
    handleOpenDeleteNodeConfirm,
    handleOpenDeleteSubConfirm,
    handleAddRule,
    handleOpenDeleteRuleConfirm,
    handleToggleAdblock,
    handleToggleMcp,
  }
}
