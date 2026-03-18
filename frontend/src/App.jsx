import { useEffect, useMemo, useRef, useState } from 'react'
import {
  ArrowDown,
  ArrowUp,
  Cat,
  Check,
  CircleAlert,
  CircleX,
  Globe,
  Info,
  LoaderCircle,
  Play,
  Plus,
  Power,
  Radio,
  RefreshCw,
  Rss,
  Server,
  Shield,
  Trash2,
  X,
  Zap,
} from 'lucide-react'

const API_HEADERS = { 'Content-Type': 'application/json' }
const POLL_INTERVAL = 3000
const CONNECTIVITY_SITES = [
  { name: 'Google', url: 'https://www.google.com' },
  { name: 'GitHub', url: 'https://github.com' },
  { name: 'YouTube', url: 'https://www.youtube.com' },
  { name: 'Bilibili', url: 'https://www.bilibili.com' },
]
const EMPTY_NODE_FORM = {
  tag: '',
  server: '',
  server_port: 443,
  password: '',
  sni: '',
  cipher: '2022-blake3-aes-128-gcm',
  skip_cert_verify: false,
}
const CIPHER_OPTIONS = [
  '2022-blake3-aes-128-gcm',
  '2022-blake3-aes-256-gcm',
  '2022-blake3-chacha20-poly1305',
  'aes-128-gcm',
  'aes-256-gcm',
  'chacha20-ietf-poly1305',
]

function classNames(...items) {
  return items.filter(Boolean).join(' ')
}

function formatUptime(seconds) {
  if (!seconds) return '--'
  const hrs = Math.floor(seconds / 3600)
  const mins = Math.floor((seconds % 3600) / 60)
  const secs = Math.floor(seconds % 60)
  if (hrs > 0) return `${hrs}h ${mins}m`
  if (mins > 0) return `${mins}m ${secs}s`
  return `${secs}s`
}

function formatSpeed(bytes) {
  if (!bytes) return '0 B/s'
  const units = ['B/s', 'KB/s', 'MB/s', 'GB/s']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** index
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[index]}`
}

function getDelayTone(delay) {
  if (delay === undefined || delay === null) return 'neutral'
  if (delay < 0) return 'timeout'
  if (delay < 80) return 'fast'
  if (delay < 180) return 'medium'
  return 'slow'
}

function formatDelay(delay) {
  if (delay === undefined || delay === null) return '--'
  if (delay < 0) return '超时'
  return `${delay} ms`
}

function protocolLabel(type) {
  const map = {
    hysteria2: 'hysteria2',
    anytls: 'anytls',
    shadowsocks: 'shadowsocks',
    ss: 'shadowsocks',
  }
  return map[type] || type || 'unknown'
}

function maskSubscription(url) {
  try {
    const parsed = new URL(url)
    const compactPath = parsed.pathname.length > 12 ? `...${parsed.pathname.slice(-8)}` : parsed.pathname
    return `${parsed.hostname}${compactPath || ''}`
  } catch {
    return url.length > 28 ? `${url.slice(0, 24)}...` : url
  }
}

function validateSubscriptionUrl(url) {
  if (!url || !url.trim()) return '订阅链接不能为空'
  if (url.length > 4096) return '订阅链接过长'
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      return '订阅链接必须使用 HTTP 或 HTTPS 协议'
    }
    if (!parsed.hostname) return '订阅链接缺少有效的主机名'
  } catch {
    return '无效的订阅链接格式'
  }
  return null
}

function validateNodeTag(tag) {
  if (!tag || !tag.trim()) return '节点名称不能为空'
  if (tag.length > 64) return '节点名称不能超过 64 个字符'
  if (!/^[a-zA-Z0-9\-_\s]+$/.test(tag)) return '节点名称只能包含字母、数字、空格、下划线和连字符'
  return null
}

function validateServer(server) {
  if (!server || !server.trim()) return '服务器地址不能为空'
  if (server.length > 253) return '服务器地址过长'
  if (/\s/.test(server)) return '服务器地址不能包含空格'
  return null
}

function validatePort(port) {
  const num = Number(port)
  if (!Number.isInteger(num) || num <= 0) return '端口号必须为正整数'
  if (num > 65535) return '端口号超出范围'
  return null
}

function validatePassword(password) {
  if (!password || !password.trim()) return '密码不能为空'
  if (password.length < 4) return '密码太短（至少 4 个字符）'
  if (password.length > 256) return '密码过长（最多 256 个字符）'
  return null
}

function Button({ children, tone = 'default', size = 'md', icon, loading, className, ...props }) {
  return (
    <button className={classNames('btn', `btn-${tone}`, `btn-${size}`, className)} {...props}>
      {loading ? <LoaderCircle className="spin" size={size === 'sm' ? 12 : 14} /> : icon}
      <span>{children}</span>
    </button>
  )
}

function SectionCard({ header, children, className, bodyClassName }) {
  return (
    <section className={classNames('panel-card', className)}>
      {header}
      <div className={classNames('panel-card-body', bodyClassName)}>{children}</div>
    </section>
  )
}

function ConfirmModal({ open, title, message, onCancel, onConfirm }) {
  if (!open) return null
  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal-card modal-confirm" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <CircleAlert size={18} className="icon-warning" />
            <h3>{title}</h3>
          </div>
          <button className="icon-button" onClick={onCancel}>
            <X size={16} />
          </button>
        </div>
        <p className="modal-message">{message}</p>
        <div className="modal-actions">
          <Button tone="ghost" size="sm" onClick={onCancel}>取消</Button>
          <Button tone="danger" size="sm" onClick={onConfirm}>确认</Button>
        </div>
      </div>
    </div>
  )
}

function NodeModal({ open, nodeType, setNodeType, form, setForm, loading, onClose, onSubmit }) {
  if (!open) return null

  const canSubmit = form.tag.trim() && form.server.trim() && form.server_port && form.password.trim()

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-card" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Plus size={18} className="icon-accent" />
            <h3>添加节点</h3>
          </div>
          <button className="icon-button" onClick={onClose}>
            <X size={16} />
          </button>
        </div>

        <div className="tab-row">
          {['hysteria2', 'anytls', 'ss'].map((value) => (
            <button
              key={value}
              className={classNames('tab-button', nodeType === value && 'active')}
              onClick={() => setNodeType(value)}
            >
              {value === 'ss' ? 'Shadowsocks' : value === 'anytls' ? 'AnyTLS' : 'Hysteria2'}
            </button>
          ))}
        </div>

        <div className="form-grid single">
          <label className="field">
            <span>节点名称</span>
            <input value={form.tag} onChange={(event) => setForm((prev) => ({ ...prev, tag: event.target.value }))} placeholder="例如：我的节点" />
          </label>
        </div>

        <div className="form-grid two">
          <label className="field">
            <span>服务器地址</span>
            <input value={form.server} onChange={(event) => setForm((prev) => ({ ...prev, server: event.target.value }))} placeholder="example.com" />
          </label>
          <label className="field">
            <span>端口</span>
            <input
              type="number"
              value={form.server_port}
              onChange={(event) => setForm((prev) => ({ ...prev, server_port: Number(event.target.value || 0) }))}
              placeholder="443"
            />
          </label>
        </div>

        {nodeType === 'ss' ? (
          <div className="form-grid single">
            <label className="field">
              <span>加密方式</span>
              <select value={form.cipher} onChange={(event) => setForm((prev) => ({ ...prev, cipher: event.target.value }))}>
                {CIPHER_OPTIONS.map((cipher) => (
                  <option key={cipher} value={cipher}>{cipher}</option>
                ))}
              </select>
            </label>
          </div>
        ) : (
          <div className="form-grid single">
            <label className="field">
              <span>SNI（可选）</span>
              <input value={form.sni} onChange={(event) => setForm((prev) => ({ ...prev, sni: event.target.value }))} placeholder="留空使用服务器地址" />
            </label>
          </div>
        )}

        {nodeType !== 'ss' && (
          <div className="form-grid single">
            <label className="field checkbox-field">
              <input
                type="checkbox"
                checked={form.skip_cert_verify}
                onChange={(event) => setForm((prev) => ({ ...prev, skip_cert_verify: event.target.checked }))}
              />
              <span>跳过证书验证（不推荐）</span>
            </label>
          </div>
        )}

        <div className="form-grid single">
          <label className="field">
            <span>密码</span>
            <input value={form.password} onChange={(event) => setForm((prev) => ({ ...prev, password: event.target.value }))} placeholder="密码" />
          </label>
        </div>

        <Button tone="primary" loading={loading} icon={<Plus size={14} />} disabled={!canSubmit || loading} onClick={onSubmit}>
          添加 {nodeType === 'ss' ? 'Shadowsocks' : nodeType === 'anytls' ? 'AnyTLS' : 'Hysteria2'} 节点
        </Button>
      </div>
    </div>
  )
}

export default function App() {
  const [status, setStatus] = useState({ running: false, pid: null, uptime_secs: null })
  const [subs, setSubs] = useState([])
  const [nodes, setNodes] = useState([])
  const [proxies, setProxies] = useState({})
  const [delays, setDelays] = useState({})
  const [testingNodes, setTestingNodes] = useState({})
  const [testingGroup, setTestingGroup] = useState('')
  const [traffic, setTraffic] = useState({})
  const [connectivityResults, setConnectivityResults] = useState({})
  const [testingConnectivity, setTestingConnectivity] = useState(false)
  const [currentTestingSite, setCurrentTestingSite] = useState(null)
  const [versionInfo, setVersionInfo] = useState({ current: '', latest: null, has_update: false })
  const [upgrading, setUpgrading] = useState(false)
  const [newSubUrl, setNewSubUrl] = useState('')
  const [nodeForm, setNodeForm] = useState(EMPTY_NODE_FORM)
  const [nodeType, setNodeType] = useState('hysteria2')
  const [showNodeModal, setShowNodeModal] = useState(false)
  const [loadingAction, setLoadingAction] = useState('')
  const [toasts, setToasts] = useState([])
  const [confirmState, setConfirmState] = useState({ open: false, title: '', message: '', onConfirm: null })

  const toastIdRef = useRef(0)
  const stopConnectivityRef = useRef(false)
  const trafficWsRef = useRef(null)
  const shownWarningRef = useRef(null)

  const clashApiBase = useMemo(() => `http://${window.location.hostname}:6262`, [])

  const selectorGroups = useMemo(() => {
    const groups = {}
    Object.entries(proxies || {}).forEach(([name, proxy]) => {
      if (proxy?.type === 'Selector') groups[name] = proxy
    })
    return groups
  }, [proxies])

  const primaryGroupName = selectorGroups.proxy ? 'proxy' : Object.keys(selectorGroups)[0]
  const primaryGroup = primaryGroupName ? selectorGroups[primaryGroupName] : null

  const nodeMetaMap = useMemo(() => {
    const map = new Map()
    nodes.forEach((node) => map.set(node.tag, node))
    return map
  }, [nodes])

  const currentNodeMeta = primaryGroup?.now ? nodeMetaMap.get(primaryGroup.now) : null

  const showToast = (message, tone = 'info') => {
    const id = ++toastIdRef.current
    setToasts((prev) => [...prev, { id, message, tone }])
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((item) => item.id !== id))
    }, 3500)
  }

  const openConfirm = (title, message, onConfirm) => {
    setConfirmState({ open: true, title, message, onConfirm })
  }

  const closeConfirm = () => {
    setConfirmState({ open: false, title: '', message: '', onConfirm: null })
  }

  const apiCall = async (endpoint, options = {}, action = '') => {
    setLoadingAction(action)
    try {
      const response = await fetch(`/api/${endpoint}`, { headers: API_HEADERS, ...options })
      const payload = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } finally {
      setLoadingAction('')
    }
  }

  const fetchStatus = async () => {
    try {
      const response = await fetch('/api/status')
      const payload = await response.json()
      if (payload.success && payload.data) {
        setStatus(payload.data)
        if (payload.data.warning && shownWarningRef.current !== payload.data.warning) {
          shownWarningRef.current = payload.data.warning
          showToast(payload.data.warning, 'error')
        }
      }
    } catch {
      // ignore
    }
  }

  const fetchSubs = async () => {
    try {
      const response = await fetch('/api/subs')
      const payload = await response.json()
      if (payload.success && payload.data) setSubs(payload.data)
    } catch {
      // ignore
    }
  }

  const fetchNodes = async () => {
    try {
      const response = await fetch('/api/nodes')
      const payload = await response.json()
      if (payload.success && payload.data) setNodes(payload.data)
    } catch {
      // ignore
    }
  }

  const fetchProxies = async () => {
    try {
      const response = await fetch(`${clashApiBase}/proxies`)
      const payload = await response.json()
      setProxies(payload.proxies || {})
    } catch {
      setProxies({})
    }
  }

  const fetchVersion = async () => {
    try {
      const response = await fetch('/api/version')
      const payload = await response.json()
      if (payload.success && payload.data) {
        setVersionInfo(payload.data)
        return payload.data
      }
    } catch {
      // ignore
    }
    return null
  }

  const closeSockets = () => {
    if (trafficWsRef.current) {
      trafficWsRef.current.close()
      trafficWsRef.current = null
    }
  }

  const connectTrafficWs = () => {
    if (trafficWsRef.current || !status.running) return
    const ws = new WebSocket(`ws://${window.location.hostname}:6262/traffic`)
    trafficWsRef.current = ws
    ws.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data)
        setTraffic({ up: data.up, down: data.down })
      } catch {
        // ignore
      }
    }
    ws.onclose = () => {
      trafficWsRef.current = null
      if (status.running) window.setTimeout(() => connectTrafficWs(), 3000)
    }
  }

  useEffect(() => {
    fetchStatus()
    fetchSubs()
    fetchNodes()
    fetchVersion()

    const timer = window.setInterval(() => {
      fetchStatus()
      fetchSubs()
      fetchNodes()
    }, POLL_INTERVAL)

    return () => {
      window.clearInterval(timer)
      closeSockets()
    }
  }, [])

  useEffect(() => {
    if (status.running) {
      fetchProxies()
      connectTrafficWs()
      const proxyTimer = window.setInterval(fetchProxies, POLL_INTERVAL)
      return () => window.clearInterval(proxyTimer)
    } else {
      setTraffic({})
      setProxies({})
      closeSockets()
    }
  }, [status.running])

  const testDelay = async (nodeName) => {
    setTestingNodes((prev) => ({ ...prev, [nodeName]: true }))
    try {
      const response = await fetch(`${clashApiBase}/proxies/${encodeURIComponent(nodeName)}/delay?timeout=3000&url=http://www.gstatic.com/generate_204`)
      if (!response.ok) {
        setDelays((prev) => ({ ...prev, [nodeName]: -1 }))
        return
      }
      const payload = await response.json()
      setDelays((prev) => ({ ...prev, [nodeName]: payload.delay > 0 ? payload.delay : -1 }))
    } catch {
      setDelays((prev) => ({ ...prev, [nodeName]: -1 }))
    } finally {
      setTestingNodes((prev) => {
        const next = { ...prev }
        delete next[nodeName]
        return next
      })
    }
  }

  const testGroupDelays = async (groupName, nodeNames) => {
    setTestingGroup(groupName)
    await Promise.all([...new Set(nodeNames)].map((name) => testDelay(name)))
    setTestingGroup('')
  }

  const handleSwitchProxy = async (groupName, nodeName) => {
    try {
      await fetch(`${clashApiBase}/proxies/${encodeURIComponent(groupName)}`, {
        method: 'PUT',
        headers: API_HEADERS,
        body: JSON.stringify({ name: nodeName }),
      })
      await fetchProxies()
      fetch('/api/last-proxy', {
        method: 'POST',
        headers: API_HEADERS,
        body: JSON.stringify({ group: groupName, name: nodeName }),
      }).catch(() => {})
      showToast(`已切换到 ${nodeName}`, 'success')
    } catch {
      showToast('切换节点失败', 'error')
    }
  }

  const handleToggleService = async () => {
    try {
      if (status.running) {
        await apiCall('service/stop', { method: 'POST' }, 'stop')
        setDelays({})
        setConnectivityResults({})
        showToast('服务已停止', 'success')
      } else {
        await apiCall('service/start', { method: 'POST' }, 'start')
        showToast('服务已启动', 'success')
      }
      await fetchStatus()
    } catch (error) {
      showToast(error.message, 'error')
    }
  }

  const handleAddSubscription = async () => {
    const error = validateSubscriptionUrl(newSubUrl.trim())
    if (error) {
      showToast(error, 'error')
      return
    }
    try {
      await apiCall('subs', { method: 'POST', body: JSON.stringify({ url: newSubUrl.trim() }) }, 'addSub')
      setNewSubUrl('')
      setDelays({})
      await fetchSubs()
      showToast('订阅已添加', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }

  const handleDeleteSubscription = async (url) => {
    try {
      await apiCall('subs', { method: 'DELETE', body: JSON.stringify({ url }) }, 'deleteSub')
      await fetchSubs()
      setDelays({})
      showToast('订阅已删除', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }

  const handleRefreshSubscriptions = async () => {
    try {
      await apiCall('subs/refresh', { method: 'POST' }, 'refreshSubs')
      await fetchSubs()
      setConnectivityResults({})
      setDelays({})
      showToast('订阅已刷新', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }

  const handleAddNode = async () => {
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
      if (nodeForm.sni.trim()) payload.sni = nodeForm.sni.trim()
      payload.skip_cert_verify = nodeForm.skip_cert_verify
    }

    try {
      await apiCall('nodes', { method: 'POST', body: JSON.stringify(payload) }, 'addNode')
      setShowNodeModal(false)
      setNodeForm(EMPTY_NODE_FORM)
      await fetchNodes()
      setDelays({})
      showToast('节点已添加', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }

  const handleDeleteNode = async (tag) => {
    try {
      await apiCall('nodes', { method: 'DELETE', body: JSON.stringify({ tag }) }, 'deleteNode')
      await fetchNodes()
      setDelays({})
      showToast('节点已删除', 'success')
    } catch (error) {
      showToast(error.message, 'error')
    }
  }

  const testSingleSite = async (site) => {
    setCurrentTestingSite(site.name)
    try {
      const response = await fetch('/api/connectivity', {
        method: 'POST',
        headers: API_HEADERS,
        body: JSON.stringify({ url: site.url }),
      })
      const payload = await response.json()
      setConnectivityResults((prev) => ({ ...prev, [site.name]: payload.success ? payload.data : { success: false } }))
    } catch {
      setConnectivityResults((prev) => ({ ...prev, [site.name]: { success: false } }))
    } finally {
      setCurrentTestingSite(null)
    }
  }

  const testAllConnectivity = async () => {
    setTestingConnectivity(true)
    stopConnectivityRef.current = false
    setConnectivityResults({})
    for (const site of CONNECTIVITY_SITES) {
      if (stopConnectivityRef.current) break
      await testSingleSite(site)
    }
    setTestingConnectivity(false)
    stopConnectivityRef.current = false
  }

  const stopConnectivity = () => {
    stopConnectivityRef.current = true
    setTestingConnectivity(false)
    setCurrentTestingSite(null)
    showToast('测试已停止', 'info')
  }

  const handleUpgradeClick = async () => {
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
    openConfirm('更新确认', `确定要从 ${versionInfo.current} 更新到 ${targetVersion} 吗？更新过程中服务会短暂中断。`, async () => {
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
              if (versionPayload.success && versionPayload.data?.current !== versionInfo.current) {
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
  }

  const currentNodeDelay = primaryGroup?.now ? delays[primaryGroup.now] : undefined
  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand">
          <Cat size={20} className="brand-icon" />
          <span className="brand-name">Miao</span>
        </div>
        <div className="topbar-spacer" />
        <div className={classNames('run-badge', status.running ? 'running' : 'stopped')}>
          <span className="run-dot" />
          {status.running ? '运行中' : '已停止'}
        </div>
        <button className={classNames('version-chip', versionInfo.has_update && 'has-update')} onClick={handleUpgradeClick} disabled={upgrading || status.initializing}>
          {upgrading && <LoaderCircle size={12} className="spin" />}
          {!upgrading && versionInfo.has_update && <span className="version-dot" />}
          <span>{versionInfo.has_update ? versionInfo.latest : versionInfo.current || 'v--'}</span>
        </button>
      </header>

      <main className="workspace">
        <SectionCard className="status-card" bodyClassName="status-card-body" header={null}>
          <div className="status-left-wrap">
            <div className="status-pill-icon"><span className="status-pill-dot" /></div>
            <div className="status-copy">
              <div className="status-title">Sing-box {status.initializing ? '初始化中' : status.running ? '运行中' : '已停止'}</div>
              <div className="status-subtitle">{status.running ? `PID: ${status.pid ?? '--'} · 运行时长: ${formatUptime(status.uptime_secs)}` : status.initializing ? '正在获取订阅并启动服务…' : '等待启动服务'}</div>
            </div>
          </div>

          <div className="traffic-chip">
            <div className="traffic-item"><ArrowUp size={14} className="traffic-icon up" /><span>{formatSpeed(traffic.up)}</span></div>
            <div className="traffic-item"><ArrowDown size={14} className="traffic-icon down" /><span>{formatSpeed(traffic.down)}</span></div>
          </div>

          <div className="status-card-spacer" />
          <Button tone={status.running ? 'danger' : 'success'} icon={<Power size={14} />} loading={loadingAction === 'start' || loadingAction === 'stop' || status.initializing} disabled={loadingAction === 'start' || loadingAction === 'stop' || status.initializing} onClick={handleToggleService}>
            {status.running ? '停止服务' : '启动服务'}
          </Button>
        </SectionCard>

        <div className="content-grid">
          <div className="left-column">
            <SectionCard
              className="proxy-card"
              bodyClassName="panel-body-tight"
              header={
                <div className="section-header">
                  <div className="section-title-wrap"><Radio size={14} className="section-icon" /><span>代理节点选择</span></div>
                  <Button tone="secondary" size="sm" icon={<Zap size={12} />} loading={testingGroup === primaryGroupName} disabled={!primaryGroup || !status.running} onClick={() => primaryGroup && testGroupDelays(primaryGroupName, primaryGroup.all)}>
                    测试延迟
                  </Button>
                </div>
              }
            >
              <button className="current-node-banner" onClick={() => primaryGroup?.now && testDelay(primaryGroup.now)} disabled={!primaryGroup?.now || Boolean(testingNodes[primaryGroup?.now])}>
                <div className="banner-icon-wrap"><span className="banner-dot" /></div>
                <div className="banner-copy">
                  <span className="banner-label">当前节点</span>
                  <strong>{primaryGroup?.now || '未选择'}</strong>
                  <span className="banner-meta">
                    {currentNodeMeta
                      ? `${currentNodeMeta.server}:${currentNodeMeta.server_port} · ${protocolLabel(currentNodeMeta.node_type)}`
                      : primaryGroup ? `来自代理组 ${primaryGroupName}` : '等待服务启动'}
                  </span>
                </div>
                <div className={classNames('banner-delay', getDelayTone(currentNodeDelay))}>
                  {testingNodes[primaryGroup?.now] ? <LoaderCircle size={20} className="spin" /> : <strong>{currentNodeDelay !== undefined && currentNodeDelay >= 0 ? currentNodeDelay : '--'}</strong>}
                  <span>ms</span>
                </div>
              </button>

              <div className="proxy-grid-wrap">
                {primaryGroup ? (
                  <div className="proxy-grid">
                    {primaryGroup.all.map((nodeName) => {
                      const delay = delays[nodeName]
                      const isActive = primaryGroup.now === nodeName
                      const isTesting = Boolean(testingNodes[nodeName])
                      return (
                        <div key={nodeName} className={classNames('proxy-tile', isActive && 'active')} onClick={() => !isTesting && handleSwitchProxy(primaryGroupName, nodeName)}>
                          <div className="proxy-tile-top">
                            {isActive
                              ? <div className="proxy-tag"><span className="proxy-tag-dot" /><span>{nodeName}</span></div>
                              : <span className="proxy-node-name">{nodeName}</span>}
                          </div>
                          <button className={classNames('proxy-test-chip', getDelayTone(delay))} onClick={(event) => { event.stopPropagation(); testDelay(nodeName); }} disabled={isTesting}>
                            {isTesting ? <LoaderCircle size={10} className="spin" /> : <Zap size={10} />}
                            <span>{isTesting ? '测试中…' : formatDelay(delay)}</span>
                          </button>
                        </div>
                      )
                    })}
                    <button className="proxy-tile add-tile" onClick={() => setShowNodeModal(true)}>
                      <Plus size={13} />
                      <span>添加节点</span>
                    </button>
                  </div>
                ) : <div className="empty-block">服务未运行，暂时无法读取代理组。</div>}
              </div>
            </SectionCard>

          </div>

          <div className="right-column">
            <SectionCard
              bodyClassName="panel-body-tight"
              header={<div className="section-header"><div className="section-title-wrap"><Server size={14} className="section-icon" /><span>手动节点</span><span className="counter-pill">{nodes.length}</span></div><Button tone="secondary" size="sm" icon={<Plus size={12} />} onClick={() => setShowNodeModal(true)}>添加</Button></div>}
            >
              <div className="list-stack">
                {nodes.length === 0 ? <div className="empty-block">暂无手动节点</div> : nodes.map((node) => (
                  <div key={node.tag} className="list-row">
                    <Shield size={13} className="list-leading-icon" />
                    <div className="list-row-content">
                      <div className="list-row-title">{node.tag}</div>
                      <div className="list-row-meta">{node.server}:{node.server_port} · {protocolLabel(node.node_type)}</div>
                    </div>
                    <button className="icon-button subtle" onClick={() => openConfirm('删除节点', `确定要删除节点 “${node.tag}” 吗？`, () => handleDeleteNode(node.tag))}><Trash2 size={13} /></button>
                  </div>
                ))}
              </div>
            </SectionCard>

            <SectionCard
              bodyClassName="panel-body-tight"
              header={<div className="section-header"><div className="section-title-wrap"><Rss size={14} className="section-icon" /><span>订阅管理</span></div><Button tone="secondary" size="sm" icon={<RefreshCw size={12} />} loading={loadingAction === 'refreshSubs'} disabled={subs.length === 0 || loadingAction === 'refreshSubs' || status.initializing} onClick={handleRefreshSubscriptions}>刷新</Button></div>}
            >
              <div className="list-stack">
                {subs.length === 0 ? <div className="empty-block">暂无订阅</div> : subs.map((sub) => (
                  <div key={sub.url} className="list-row">
                    <div className={classNames('status-icon-badge', sub.success ? 'success' : 'error')}>{sub.success ? <Check size={12} /> : <CircleX size={12} />}</div>
                    <div className="list-row-content">
                      <div className="list-row-title">{maskSubscription(sub.url)}</div>
                      <div className={classNames('list-row-meta', !sub.success && 'error')}>{sub.success ? `${sub.node_count} 个节点` : sub.error || '获取失败'}</div>
                    </div>
                    <button className="icon-button subtle" onClick={() => openConfirm('删除订阅', `确定要删除此订阅吗？\n${sub.url}`, () => handleDeleteSubscription(sub.url))}><X size={13} /></button>
                  </div>
                ))}
                <div className="subscription-add-row">
                  <input value={newSubUrl} onChange={(event) => setNewSubUrl(event.target.value)} onKeyDown={(event) => event.key === 'Enter' && handleAddSubscription()} placeholder="粘贴订阅链接..." />
                  <Button tone="secondary" size="sm" icon={<Plus size={12} />} loading={loadingAction === 'addSub'} onClick={handleAddSubscription}>添加</Button>
                </div>
              </div>
            </SectionCard>

            <SectionCard
              bodyClassName="panel-body-tight"
              header={<div className="section-header"><div className="section-title-wrap"><Globe size={14} className="section-icon" /><span>连通性测试</span></div><Button tone="secondary" size="sm" icon={<Play size={11} />} loading={testingConnectivity} disabled={status.initializing} onClick={testingConnectivity ? stopConnectivity : testAllConnectivity}>{testingConnectivity ? '停止测试' : '开始测试'}</Button></div>}
            >
              <div className="connectivity-grid">
                {CONNECTIVITY_SITES.map((site) => {
                  const result = connectivityResults[site.name]
                  const tone = result ? (result.success ? getDelayTone(result.latency_ms) : 'timeout') : ''
                  return (
                    <button key={site.name} className={classNames('connectivity-item', tone, currentTestingSite === site.name && 'testing')} onClick={() => !currentTestingSite && testSingleSite(site)} disabled={Boolean(currentTestingSite)}>
                      <div className="connectivity-copy">
                        <span>{site.name}</span>
                        <span>{result ? (result.success ? `${result.latency_ms}ms` : '超时') : '--'}</span>
                      </div>
                    </button>
                  )
                })}
              </div>
            </SectionCard>

          </div>
        </div>
      </main>

      <div className="toast-stack">
        {toasts.map((toast) => (
          <div key={toast.id} className={classNames('toast', toast.tone)}>
            {toast.tone === 'success' ? <Check size={14} /> : toast.tone === 'error' ? <CircleX size={14} /> : <Info size={14} />}
            <span>{toast.message}</span>
          </div>
        ))}
      </div>

      <NodeModal open={showNodeModal} nodeType={nodeType} setNodeType={setNodeType} form={nodeForm} setForm={setNodeForm} loading={loadingAction === 'addNode'} onClose={() => setShowNodeModal(false)} onSubmit={handleAddNode} />

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
