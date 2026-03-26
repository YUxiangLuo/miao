import { useEffect, useMemo, useState, useCallback } from 'react'
import { 
  TopBar,
  StatusCard,
  ProxyCard,
  NodesCard,
  SubsCard,
  ConnectivityCard,
  ConfirmModal,
  NodeModal,
  ToastStack
} from './components/index.js'
import {
  useToast,
  useApi,
  useStatus,
  useSubs,
  useNodes,
  useProxies,
  useTraffic,
  useVersion,
  useDelays,
  useConnectivity,
  usePolling
} from './hooks/index.js'
import {
  EMPTY_NODE_FORM,
  validateSubscriptionUrl,
  validateNodeTag,
  validateServer,
  validatePort,
  validatePassword,
  CONNECTIVITY_SITES
} from './utils.js'

export default function App() {
  const [loadingAction, setLoadingAction] = useState('')
  const [upgrading, setUpgrading] = useState(false)
  const [newSubUrl, setNewSubUrl] = useState('')
  const [nodeForm, setNodeForm] = useState(EMPTY_NODE_FORM)
  const [nodeType, setNodeType] = useState('hysteria2')
  const [showNodeModal, setShowNodeModal] = useState(false)
  const [confirmState, setConfirmState] = useState({ open: false, title: '', message: '', onConfirm: null })

  const { toasts, showToast } = useToast()
  const { apiCall } = useApi({ loadingAction, setLoadingAction })
  const { status, fetchStatus } = useStatus()
  const { subs, setSubs, fetchSubs } = useSubs()
  const { nodes, setNodes, fetchNodes } = useNodes()
  const { primaryGroupName, primaryGroup, fetchProxies } = useProxies(status)
  const { traffic, closeSockets } = useTraffic(status)
  const { versionInfo, setVersionInfo, fetchVersion } = useVersion()
  const { delays, testingNodes, testingGroup, testDelay, testGroupDelays, clearDelays } = useDelays()
  const { 
    connectivityResults, 
    testingConnectivity, 
    currentTestingSite,
    testSingleSite, 
    testAllConnectivity, 
    stopConnectivity,
    clearConnectivity
  } = useConnectivity()

  const clashApiBase = useMemo(() => `http://${window.location.hostname}:6262`, [])

  const nodeMetaMap = useMemo(() => {
    const map = new Map()
    nodes.forEach((node) => map.set(node.tag, node))
    return map
  }, [nodes])

  const currentNodeMeta = primaryGroup?.now ? nodeMetaMap.get(primaryGroup.now) : null

  const openConfirm = useCallback((title, message, onConfirm) => {
    setConfirmState({ open: true, title, message, onConfirm })
  }, [])

  const closeConfirm = useCallback(() => {
    setConfirmState({ open: false, title: '', message: '', onConfirm: null })
  }, [])

  // 统一轮询管理：合并所有定时任务到单个定时器
  const pollingTasks = useMemo(() => {
    const tasks = [fetchStatus, fetchSubs, fetchNodes]
    // 服务运行时才轮询 proxies
    if (status.running) {
      tasks.push(fetchProxies)
    }
    return tasks
  }, [fetchStatus, fetchSubs, fetchNodes, fetchProxies, status.running])

  // 使用统一的轮询管理（始终启用，由 tasks 数组内部决定是否执行）
  usePolling(pollingTasks, true)

  // 初始化：获取版本信息（不需要轮询）
  useEffect(() => {
    fetchVersion()
  }, [fetchVersion])

  // 清理 WebSocket 连接
  useEffect(() => {
    return () => closeSockets()
  }, [closeSockets])

  // Show warning toast when config has warning
  useEffect(() => {
    if (status.warning) {
      showToast(status.warning, 'error')
    }
  }, [status.warning, showToast])

  // Clear delays and connectivity when service stops
  useEffect(() => {
    if (!status.running) {
      clearDelays()
      clearConnectivity()
    }
  }, [status.running, clearDelays, clearConnectivity])

  const handleToggleService = useCallback(async () => {
    try {
      if (status.running) {
        await apiCall('service/stop', { method: 'POST' }, 'stop')
        clearDelays()
        clearConnectivity()
        showToast('服务已停止', 'success')
      } else {
        await apiCall('service/start', { method: 'POST' }, 'start')
        showToast('服务已启动', 'success')
      }
      await fetchStatus()
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [status.running, apiCall, clearDelays, clearConnectivity, fetchStatus, showToast])

  const handleSwitchProxy = useCallback(async (groupName, nodeName) => {
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
    } catch {
      showToast('切换节点失败', 'error')
    }
  }, [clashApiBase, fetchProxies, showToast])

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
  }, [newSubUrl, apiCall, clearDelays, fetchSubs, showToast])

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
      clearConnectivity()
      clearDelays()
      showToast('订阅已刷新', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [apiCall, clearConnectivity, clearDelays, fetchSubs, showToast])

  const handleAddNode = useCallback(async () => {
    const tagError = validateNodeTag(nodeForm.tag)
    if (tagError) {
      showToast(tagError, 'error')
      return
    }
    const serverError = validateServer(nodeForm.server)
    if (serverError) {
      showToast(serverError, 'error')
      return
    }
    const portError = validatePort(nodeForm.server_port)
    if (portError) {
      showToast(portError, 'error')
      return
    }
    const passwordError = validatePassword(nodeForm.password)
    if (passwordError) {
      showToast(passwordError, 'error')
      return
    }

    const payload = {
      node_type: nodeType,
      tag: nodeForm.tag.trim(),
      server: nodeForm.server.trim(),
      server_port: nodeForm.server_port,
      password: nodeForm.password.trim(),
    }
    if (nodeType === 'ss') {
      payload.cipher = nodeForm.cipher
    } else {
      if (nodeForm.sni?.trim()) payload.sni = nodeForm.sni.trim()
      payload.skip_cert_verify = nodeForm.skip_cert_verify
    }

    try {
      await apiCall('nodes', { method: 'POST', body: JSON.stringify(payload) }, 'addNode')
      setShowNodeModal(false)
      setNodeForm(EMPTY_NODE_FORM)
      await fetchNodes()
      clearDelays()
      showToast('节点已添加', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }, [nodeForm, nodeType, apiCall, clearDelays, fetchNodes, showToast])

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

  const handleTestSingleSite = useCallback((site) => {
    testSingleSite(site)
  }, [testSingleSite])

  const handleTestAllConnectivity = useCallback(() => {
    testAllConnectivity(CONNECTIVITY_SITES)
  }, [testAllConnectivity])

  const handleUpgradeClick = useCallback(async () => {
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
  }, [versionInfo, fetchVersion, showToast, openConfirm])

  const handleOpenDeleteNodeConfirm = useCallback((tag) => {
    openConfirm('删除节点', `确定要删除节点 "${tag}" 吗？`, () => handleDeleteNode(tag))
  }, [openConfirm, handleDeleteNode])

  const handleOpenDeleteSubConfirm = useCallback((url) => {
    openConfirm('删除订阅', `确定要删除此订阅吗？\n${url}`, () => handleDeleteSubscription(url))
  }, [openConfirm, handleDeleteSubscription])

  return (
    <div className="shell">
      <TopBar 
        status={status} 
        versionInfo={versionInfo} 
        upgrading={upgrading} 
        onUpgradeClick={handleUpgradeClick} 
      />

      <main className="workspace">
        <StatusCard 
          status={status} 
          traffic={traffic} 
          loadingAction={loadingAction} 
          onToggleService={handleToggleService} 
        />

        <div className="content-grid">
          <div className="left-column">
            <ProxyCard
              status={status}
              primaryGroup={primaryGroup}
              primaryGroupName={primaryGroupName}
              currentNodeMeta={currentNodeMeta}
              delays={delays}
              testingNodes={testingNodes}
              testingGroup={testingGroup}
              onTestDelay={handleTestDelay}
              onTestGroupDelays={handleTestGroupDelays}
              onSwitchProxy={handleSwitchProxy}
              onOpenAddNode={() => setShowNodeModal(true)}
            />
          </div>

          <div className="right-column">
            <NodesCard 
              nodes={nodes} 
              onDeleteNode={handleOpenDeleteNodeConfirm} 
              onOpenAddNode={() => setShowNodeModal(true)} 
            />

            <SubsCard
              subs={subs}
              newSubUrl={newSubUrl}
              setNewSubUrl={setNewSubUrl}
              loadingAction={loadingAction}
              onAddSub={handleAddSubscription}
              onDeleteSub={handleOpenDeleteSubConfirm}
              onRefreshSubs={handleRefreshSubscriptions}
              isInitializing={status.initializing}
            />

            <ConnectivityCard
              connectivityResults={connectivityResults}
              testingConnectivity={testingConnectivity}
              currentTestingSite={currentTestingSite}
              status={status}
              onTestAll={handleTestAllConnectivity}
              onStopTest={stopConnectivity}
              onTestSingleSite={handleTestSingleSite}
            />
          </div>
        </div>
      </main>

      <ToastStack toasts={toasts} />

      <NodeModal 
        open={showNodeModal} 
        nodeType={nodeType} 
        setNodeType={setNodeType} 
        form={nodeForm} 
        setForm={setNodeForm} 
        loading={loadingAction === 'addNode'} 
        onClose={() => setShowNodeModal(false)} 
        onSubmit={handleAddNode} 
      />

      <ConfirmModal
        open={confirmState.open}
        title={confirmState.title}
        message={confirmState.message}
        onCancel={closeConfirm}
        onConfirm={() => {
          const action = confirmState.onConfirm
          closeConfirm()
          action?.()
        }}
      />
    </div>
  )
}
