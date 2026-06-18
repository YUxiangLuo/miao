import { useEffect, useMemo, useState } from 'react'
import { X, CircleAlert, Plus, Activity, ArrowDown, ArrowUp, Network, RefreshCw, Route, Search, Trash2 } from 'lucide-react'
import { Button } from './ui.jsx'
import { 
  classNames, 
  CIPHER_OPTIONS, 
  CLIENT_FINGERPRINT_OPTIONS,
  HYSTERIA2_OBFS_OPTIONS,
  NODE_TYPE_OPTIONS,
  PACKET_ENCODING_OPTIONS,
  TRANSPORT_OPTIONS,
  TUIC_CONGESTION_OPTIONS,
  TUIC_UDP_RELAY_OPTIONS,
  VMESS_CIPHER_OPTIONS,
  formatBytes,
  nodeTypeDefaults,
} from '../utils.js'

export function ConfirmModal({ open, title, message, onCancel, onConfirm }) {
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

export function NodeModal({ open, nodeType, setNodeType, form, setForm, loading, onClose, onSubmit }) {
  if (!open) return null

  const activeLabel = NODE_TYPE_OPTIONS.find((option) => option.value === nodeType)?.label || nodeType
  const requiresPassword = ['hysteria2', 'anytls', 'ss', 'trojan', 'tuic'].includes(nodeType)
  const requiresUuid = ['vmess', 'vless', 'tuic'].includes(nodeType)
  const supportsTransport = ['vmess', 'vless', 'trojan'].includes(nodeType)
  const showsTlsToggle = ['vmess', 'vless'].includes(nodeType)
  const showsTlsFields = nodeType !== 'ss' && (!showsTlsToggle || form.tls_enabled || form.reality_public_key.trim())
  const pathTransport = ['ws', 'http', 'h2'].includes(form.transport_type)

  const canSubmit = form.tag.trim()
    && form.server.trim()
    && form.server_port
    && (!requiresPassword || form.password.trim())
    && (!requiresUuid || form.uuid.trim())
    && (nodeType !== 'hysteria2' || !form.obfs_type || form.obfs_password.trim())

  return (
    <div className="modal-overlay">
      <div className="modal-card node-modal" onClick={(event) => event.stopPropagation()}>
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
          {NODE_TYPE_OPTIONS.map(({ value, label }) => (
            <button
              key={value}
              className={classNames('tab-button', nodeType === value && 'active')}
              onClick={() => {
                setNodeType(value)
                setForm((prev) => ({ ...prev, ...nodeTypeDefaults(value) }))
              }}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="form-grid single">
          <label className="field">
            <span>节点名称</span>
            <input 
              value={form.tag} 
              onChange={(event) => setForm((prev) => ({ ...prev, tag: event.target.value }))} 
              placeholder="例如：我的节点" 
            />
          </label>
        </div>

        <div className="form-grid two">
          <label className="field">
            <span>服务器地址</span>
            <input 
              value={form.server} 
              onChange={(event) => setForm((prev) => ({ ...prev, server: event.target.value }))} 
              placeholder="example.com" 
            />
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

        {nodeType === 'ss' && (
          <div className="form-grid single">
            <label className="field">
              <span>加密方式</span>
              <select 
                value={form.cipher} 
                onChange={(event) => setForm((prev) => ({ ...prev, cipher: event.target.value }))}
              >
                {CIPHER_OPTIONS.map((cipher) => (
                  <option key={cipher} value={cipher}>{cipher}</option>
                ))}
              </select>
            </label>
          </div>
        )}

        {nodeType === 'vmess' && (
          <div className="form-grid two">
            <label className="field">
              <span>VMess security</span>
              <select
                value={form.vmess_cipher}
                onChange={(event) => setForm((prev) => ({ ...prev, vmess_cipher: event.target.value }))}
              >
                {VMESS_CIPHER_OPTIONS.map((cipher) => (
                  <option key={cipher} value={cipher}>{cipher}</option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>Alter ID</span>
              <input
                type="number"
                value={form.alter_id}
                onChange={(event) => setForm((prev) => ({ ...prev, alter_id: Number(event.target.value || 0) }))}
                min="0"
              />
            </label>
          </div>
        )}

        {requiresUuid && (
          <div className="form-grid single">
            <label className="field">
              <span>UUID</span>
              <input
                value={form.uuid}
                onChange={(event) => setForm((prev) => ({ ...prev, uuid: event.target.value }))}
                placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
              />
            </label>
          </div>
        )}

        {showsTlsToggle && (
          <div className="form-grid single">
            <label className="field checkbox-field">
              <input
                type="checkbox"
                checked={form.tls_enabled}
                onChange={(event) => setForm((prev) => ({ ...prev, tls_enabled: event.target.checked }))}
              />
              <span>启用 TLS</span>
            </label>
          </div>
        )}

        {nodeType === 'vless' && (
          <div className="form-grid two">
            <label className="field">
              <span>Flow</span>
              <select
                value={form.flow}
                onChange={(event) => setForm((prev) => ({ ...prev, flow: event.target.value }))}
              >
                <option value="">默认</option>
                <option value="xtls-rprx-vision">xtls-rprx-vision</option>
              </select>
            </label>
            <label className="field">
              <span>Packet encoding</span>
              <select
                value={form.packet_encoding}
                onChange={(event) => setForm((prev) => ({ ...prev, packet_encoding: event.target.value }))}
              >
                {PACKET_ENCODING_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
            </label>
          </div>
        )}

        {nodeType === 'vmess' && (
          <div className="form-grid single">
            <label className="field">
              <span>Packet encoding</span>
              <select
                value={form.packet_encoding}
                onChange={(event) => setForm((prev) => ({ ...prev, packet_encoding: event.target.value }))}
              >
                {PACKET_ENCODING_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
            </label>
          </div>
        )}

        {showsTlsFields && (
          <>
            <div className="form-grid two">
              <label className="field">
                <span>SNI（可选）</span>
                <input
                  value={form.sni}
                  onChange={(event) => setForm((prev) => ({ ...prev, sni: event.target.value }))}
                  placeholder="留空使用服务器地址"
                />
              </label>
              <label className="field">
                <span>TLS 指纹</span>
                <select
                  value={form.client_fingerprint}
                  onChange={(event) => setForm((prev) => ({ ...prev, client_fingerprint: event.target.value }))}
                >
                  {CLIENT_FINGERPRINT_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
            </div>
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
          </>
        )}

        {nodeType === 'vless' && (
          <div className="form-grid two">
            <label className="field">
              <span>Reality public key</span>
              <input
                value={form.reality_public_key}
                onChange={(event) => {
                  const publicKey = event.target.value
                  setForm((prev) => ({
                    ...prev,
                    reality_public_key: publicKey,
                    client_fingerprint: publicKey.trim() && !prev.client_fingerprint
                      ? 'chrome'
                      : prev.client_fingerprint,
                  }))
                }}
                placeholder="可选"
              />
            </label>
            <label className="field">
              <span>Reality short ID</span>
              <input
                value={form.reality_short_id}
                onChange={(event) => setForm((prev) => ({ ...prev, reality_short_id: event.target.value }))}
                placeholder="可选"
              />
            </label>
          </div>
        )}

        {supportsTransport && (
          <>
            <div className="form-grid single">
              <label className="field">
                <span>传输层</span>
                <select
                  value={form.transport_type}
                  onChange={(event) => setForm((prev) => ({ ...prev, transport_type: event.target.value }))}
                >
                  {TRANSPORT_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
            </div>
            {pathTransport && (
              <div className="form-grid two">
                <label className="field">
                  <span>路径</span>
                  <input
                    value={form.transport_path}
                    onChange={(event) => setForm((prev) => ({ ...prev, transport_path: event.target.value }))}
                    placeholder="/ws"
                  />
                </label>
                <label className="field">
                  <span>Host</span>
                  <input
                    value={form.transport_host}
                    onChange={(event) => setForm((prev) => ({ ...prev, transport_host: event.target.value }))}
                    placeholder="可选"
                  />
                </label>
              </div>
            )}
            {form.transport_type === 'grpc' && (
              <div className="form-grid single">
                <label className="field">
                  <span>gRPC service name</span>
                  <input
                    value={form.grpc_service_name}
                    onChange={(event) => setForm((prev) => ({ ...prev, grpc_service_name: event.target.value }))}
                    placeholder="可选"
                  />
                </label>
              </div>
            )}
          </>
        )}

        {nodeType === 'hysteria2' && (
          <>
            <div className="form-grid two">
              <label className="field">
                <span>混淆类型</span>
                <select
                  value={form.obfs_type}
                  onChange={(event) => {
                    const obfsType = event.target.value
                    setForm((prev) => ({
                      ...prev,
                      obfs_type: obfsType,
                      obfs_password: obfsType ? prev.obfs_password : '',
                    }))
                  }}
                >
                  {HYSTERIA2_OBFS_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>混淆密码</span>
                <input
                  value={form.obfs_password}
                  disabled={!form.obfs_type}
                  onChange={(event) => setForm((prev) => ({ ...prev, obfs_password: event.target.value }))}
                  placeholder={form.obfs_type ? 'obfs password' : '未启用'}
                />
              </label>
            </div>
          </>
        )}

        {nodeType === 'tuic' && (
          <div className="form-grid two">
            <label className="field">
              <span>拥塞控制</span>
              <select
                value={form.tuic_congestion_control}
                onChange={(event) => setForm((prev) => ({ ...prev, tuic_congestion_control: event.target.value }))}
              >
                {TUIC_CONGESTION_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>UDP relay mode</span>
              <select
                value={form.tuic_udp_relay_mode}
                onChange={(event) => setForm((prev) => ({ ...prev, tuic_udp_relay_mode: event.target.value }))}
              >
                {TUIC_UDP_RELAY_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
            </label>
          </div>
        )}

        {nodeType === 'tuic' && (
          <div className="form-grid single">
            <label className="field checkbox-field">
              <input
                type="checkbox"
                checked={form.tuic_zero_rtt}
                onChange={(event) => setForm((prev) => ({ ...prev, tuic_zero_rtt: event.target.checked }))}
              />
              <span>启用 0-RTT</span>
            </label>
          </div>
        )}

        {requiresPassword && (
          <div className="form-grid single">
            <label className="field">
              <span>密码</span>
              <input
                value={form.password}
                onChange={(event) => setForm((prev) => ({ ...prev, password: event.target.value }))}
                placeholder="密码"
              />
            </label>
          </div>
        )}

        <Button 
          tone="primary" 
          loading={loading} 
          icon={<Plus size={14} />} 
          disabled={!canSubmit || loading} 
          onClick={onSubmit}
        >
          添加 {activeLabel} 节点
        </Button>
      </div>
    </div>
  )
}

function countBy(items, mapper) {
  return items.reduce((acc, item) => {
    const key = mapper(item) || 'unknown'
    acc[key] = (acc[key] || 0) + 1
    return acc
  }, {})
}

function topEntries(counts, limit = 5) {
  return Object.entries(counts)
    .sort((a, b) => b[1] - a[1])
    .slice(0, limit)
}

const CONNECTION_PAGE_SIZE = 20

const SORT_OPTIONS = [
  { value: 'downloadSpeed', label: '下载速度' },
  { value: 'uploadSpeed', label: '上传速度' },
  { value: 'download', label: '下载总量' },
  { value: 'upload', label: '上传总量' },
  { value: 'start', label: '连接时间' },
  { value: 'host', label: '目标' },
  { value: 'source', label: '来源' },
  { value: 'outbound', label: '出口' },
]

function processName(connection) {
  const path = connection.metadata?.processPath || ''
  return connection.metadata?.process || path.replace(/^.*[/\\]/, '') || '-'
}

function connectionTarget(connection) {
  const metadata = connection.metadata || {}
  const host = metadata.host || metadata.sniffHost || metadata.remoteDestination || metadata.destinationIP || metadata.destination
  const port = metadata.destinationPort || metadata.remoteDestinationPort
  if (!host) return 'unknown'
  return port ? `${host}:${port}` : host
}

function connectionDestination(connection) {
  const metadata = connection.metadata || {}
  return metadata.remoteDestination || metadata.destinationIP || metadata.host || metadata.sniffHost || 'unknown'
}

function connectionSource(connection) {
  const metadata = connection.metadata || {}
  const ip = connectionSourceIP(connection)
  return metadata.sourcePort ? `${ip}:${metadata.sourcePort}` : ip
}

function connectionSourceIP(connection) {
  return connection.metadata?.sourceIP || 'inner'
}

function connectionRule(connection) {
  const rule = connection.rule || '-'
  return connection.rulePayload ? `${rule} : ${connection.rulePayload}` : rule
}

function connectionOutbound(connection) {
  if (Array.isArray(connection.chains) && connection.chains.length > 0) {
    return connection.chains[0]
  }
  return connection.rule || 'direct'
}

function connectionSearchText(connection) {
  return [
    connection.id,
    connectionTarget(connection),
    connectionDestination(connection),
    connectionSource(connection),
    connectionRule(connection),
    connectionOutbound(connection),
    processName(connection),
    connection.metadata?.network,
    connection.metadata?.type,
    ...(Array.isArray(connection.chains) ? connection.chains : []),
  ].filter(Boolean).join(' ').toLowerCase()
}

function sortValue(connection, sortKey) {
  switch (sortKey) {
    case 'uploadSpeed':
      return Number(connection.uploadSpeed || 0)
    case 'download':
      return Number(connection.download || 0)
    case 'upload':
      return Number(connection.upload || 0)
    case 'start':
      return new Date(connection.start || 0).getTime()
    case 'host':
      return connectionTarget(connection)
    case 'source':
      return connectionSource(connection)
    case 'outbound':
      return connectionOutbound(connection)
    case 'downloadSpeed':
    default:
      return Number(connection.downloadSpeed || 0)
  }
}

function formatStartTime(value) {
  if (!value) return '-'
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return '-'
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function DetailRow({ label, value }) {
  return (
    <>
      <dt>{label}</dt>
      <dd title={String(value || '-')}>{value || '-'}</dd>
    </>
  )
}

export function ConnectionsModal({
  open,
  status,
  data,
  loading,
  error,
  onClose,
  onRefresh,
  onCloseConnection,
  onCloseAllConnections,
  showToast,
}) {
  const [query, setQuery] = useState('')
  const [sourceFilter, setSourceFilter] = useState('')
  const [sortKey, setSortKey] = useState('downloadSpeed')
  const [sortDesc, setSortDesc] = useState(true)
  const [page, setPage] = useState(0)
  const [selectedId, setSelectedId] = useState('')
  const [closingId, setClosingId] = useState('')
  const [closingAll, setClosingAll] = useState(false)

  useEffect(() => {
    if (open) setPage(0)
  }, [open, query, sourceFilter, sortKey, sortDesc])

  const connections = useMemo(() => {
    return Array.isArray(data?.connections) ? data.connections : []
  }, [data?.connections])
  const uploadTotal = Number(data?.uploadTotal || connections.reduce((sum, item) => sum + Number(item.upload || 0), 0))
  const downloadTotal = Number(data?.downloadTotal || connections.reduce((sum, item) => sum + Number(item.download || 0), 0))
  const uploadSpeed = connections.reduce((sum, item) => sum + Number(item.uploadSpeed || 0), 0)
  const downloadSpeed = connections.reduce((sum, item) => sum + Number(item.downloadSpeed || 0), 0)
  const networkCounts = topEntries(countBy(connections, (item) => item.metadata?.network), 4)
  const outboundCounts = topEntries(countBy(connections, connectionOutbound), 5)
  const sourceOptions = useMemo(() => {
    return [...new Set(connections.map(connectionSourceIP))].sort()
  }, [connections])
  const filteredConnections = useMemo(() => {
    const needle = query.trim().toLowerCase()
    const filtered = connections.filter((connection) => {
      if (sourceFilter && connectionSourceIP(connection) !== sourceFilter) return false
      return !needle || connectionSearchText(connection).includes(needle)
    })

    return [...filtered].sort((a, b) => {
      const aValue = sortValue(a, sortKey)
      const bValue = sortValue(b, sortKey)
      const comparison = typeof aValue === 'number' && typeof bValue === 'number'
        ? aValue - bValue
        : String(aValue).localeCompare(String(bValue))
      return sortDesc ? -comparison : comparison
    })
  }, [connections, query, sortDesc, sortKey, sourceFilter])
  const pageCount = Math.max(1, Math.ceil(filteredConnections.length / CONNECTION_PAGE_SIZE))
  const safePage = Math.min(page, pageCount - 1)
  const pageStart = safePage * CONNECTION_PAGE_SIZE
  const visibleConnections = filteredConnections.slice(pageStart, pageStart + CONNECTION_PAGE_SIZE)
  const selectedConnection = selectedId
    ? connections.find((connection) => connection.id === selectedId)
    : null

  const handleCloseSingle = async (connectionId) => {
    setClosingId(connectionId)
    try {
      await onCloseConnection(connectionId)
      if (selectedId === connectionId) setSelectedId('')
    } catch (closeError) {
      showToast?.(closeError.message || '关闭连接失败', 'error')
    } finally {
      setClosingId('')
    }
  }

  const handleCloseAll = async () => {
    setClosingAll(true)
    try {
      await onCloseAllConnections()
      setSelectedId('')
    } catch (closeError) {
      showToast?.(closeError.message || '关闭全部连接失败', 'error')
    } finally {
      setClosingAll(false)
    }
  }

  if (!open) return null

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-card connections-modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Activity size={18} className="icon-accent" />
            <h3>连接统计</h3>
          </div>
          <div className="modal-title-actions">
            <button className="icon-button" onClick={onRefresh} disabled={loading || !status.running} title="刷新">
              <RefreshCw size={16} className={loading ? 'spin' : undefined} />
            </button>
            <button className="icon-button" onClick={onClose} title="关闭">
              <X size={16} />
            </button>
          </div>
        </div>

        {!status.running ? (
          <div className="connections-empty">服务未运行，暂无连接统计。</div>
        ) : (
          <>
            <div className="connection-stat-grid">
              <div className="connection-stat">
                <span>活跃连接</span>
                <strong>{connections.length}</strong>
              </div>
              <div className="connection-stat">
                <span>当前速度</span>
                <strong>↓ {formatBytes(downloadSpeed)}/s</strong>
                <small>↑ {formatBytes(uploadSpeed)}/s</small>
              </div>
              <div className="connection-stat">
                <span>累计上传</span>
                <strong>{formatBytes(uploadTotal)}</strong>
              </div>
              <div className="connection-stat">
                <span>累计下载</span>
                <strong>{formatBytes(downloadTotal)}</strong>
              </div>
              <div className="connection-stat">
                <span>总流量</span>
                <strong>{formatBytes(uploadTotal + downloadTotal)}</strong>
              </div>
            </div>

            {error && <div className="connections-error">{error}</div>}

            <div className="connections-toolbar">
              <label className="connections-search">
                <Search size={14} />
                <input
                  type="search"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder="搜索目标、来源、规则、出口、进程"
                />
              </label>
              <select value={sourceFilter} onChange={(event) => setSourceFilter(event.target.value)}>
                <option value="">全部来源</option>
                {sourceOptions.map((source) => (
                  <option key={source} value={source}>{source}</option>
                ))}
              </select>
              <select value={sortKey} onChange={(event) => setSortKey(event.target.value)}>
                {SORT_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
              <button className="connections-tool-button" onClick={() => setSortDesc((value) => !value)}>
                {sortDesc ? '降序' : '升序'}
              </button>
              <button
                className="connections-tool-button danger"
                onClick={handleCloseAll}
                disabled={closingAll || loading || connections.length === 0}
                title="关闭全部连接"
              >
                {closingAll ? <RefreshCw size={14} className="spin" /> : <Trash2 size={14} />}
              </button>
            </div>

            <div className="connections-split">
              <div className="connections-panel">
                <div className="connections-panel-title">
                  <Network size={14} />
                  <span>协议分布</span>
                </div>
                {networkCounts.length > 0 ? networkCounts.map(([name, count]) => (
                  <div className="connection-count-row" key={name}>
                    <span>{name}</span>
                    <strong>{count}</strong>
                  </div>
                )) : <div className="connections-muted">暂无数据</div>}
              </div>

              <div className="connections-panel">
                <div className="connections-panel-title">
                  <Route size={14} />
                  <span>出口分布</span>
                </div>
                {outboundCounts.length > 0 ? outboundCounts.map(([name, count]) => (
                  <div className="connection-count-row" key={name}>
                    <span title={name}>{name}</span>
                    <strong>{count}</strong>
                  </div>
                )) : <div className="connections-muted">暂无数据</div>}
              </div>
            </div>

            <div className="connections-table">
              <div className="connections-table-header">
                <span />
                <span>目标</span>
                <span>规则 / 出口</span>
                <span>来源</span>
                <span>速度</span>
                <span>总量</span>
              </div>
              {visibleConnections.length > 0 ? visibleConnections.map((connection, index) => (
                <div
                  className={classNames('connections-table-row', selectedId === connection.id && 'active')}
                  key={connection.id || `${connectionTarget(connection)}-${index}`}
                  onClick={() => setSelectedId(connection.id)}
                >
                  <button
                    className="connection-row-close"
                    onClick={(event) => {
                      event.stopPropagation()
                      handleCloseSingle(connection.id)
                    }}
                    disabled={closingId === connection.id}
                    title="关闭连接"
                  >
                    {closingId === connection.id ? <RefreshCw size={13} className="spin" /> : <X size={13} />}
                  </button>
                  <span className="connection-host" title={connectionTarget(connection)}>
                    <strong>{connectionTarget(connection)}</strong>
                    <small>{processName(connection)} · {formatStartTime(connection.start)}</small>
                  </span>
                  <span className="connection-rule" title={`${connectionRule(connection)} → ${(connection.chains || []).join(' → ')}`}>
                    <strong>{connectionRule(connection)}</strong>
                    <small>{(connection.chains || []).length ? [...connection.chains].reverse().join(' → ') : connectionOutbound(connection)}</small>
                  </span>
                  <span title={connectionSource(connection)}>{connectionSource(connection)}</span>
                  <span>
                    <small><ArrowDown size={12} />{formatBytes(Number(connection.downloadSpeed || 0))}/s</small>
                    <small><ArrowUp size={12} />{formatBytes(Number(connection.uploadSpeed || 0))}/s</small>
                  </span>
                  <span>
                    <small><ArrowDown size={12} />{formatBytes(Number(connection.download || 0))}</small>
                    <small><ArrowUp size={12} />{formatBytes(Number(connection.upload || 0))}</small>
                  </span>
                </div>
              )) : <div className="connections-empty inline">暂无匹配连接</div>}
            </div>

            <div className="connections-pagination">
              <span>
                {filteredConnections.length === 0
                  ? '0 / 0'
                  : `${pageStart + 1}-${Math.min(pageStart + visibleConnections.length, filteredConnections.length)} / ${filteredConnections.length}`}
              </span>
              <div>
                <button className="connections-tool-button" disabled={safePage === 0} onClick={() => setPage((value) => Math.max(0, value - 1))}>上一页</button>
                <button className="connections-tool-button" disabled={safePage >= pageCount - 1} onClick={() => setPage((value) => Math.min(pageCount - 1, value + 1))}>下一页</button>
              </div>
            </div>

            {selectedConnection && (
              <div className="connection-detail-panel">
                <div className="connection-detail-title">
                  <strong>连接详情</strong>
                  <button className="icon-button subtle" onClick={() => setSelectedId('')} title="关闭详情">
                    <X size={14} />
                  </button>
                </div>
                <dl>
                  <DetailRow label="ID" value={selectedConnection.id} />
                  <DetailRow label="开始时间" value={formatStartTime(selectedConnection.start)} />
                  <DetailRow label="网络" value={`${selectedConnection.metadata?.type || '-'} / ${selectedConnection.metadata?.network || '-'}`} />
                  <DetailRow label="目标" value={connectionTarget(selectedConnection)} />
                  <DetailRow label="远端目标" value={connectionDestination(selectedConnection)} />
                  <DetailRow label="来源" value={connectionSource(selectedConnection)} />
                  <DetailRow label="规则" value={connectionRule(selectedConnection)} />
                  <DetailRow label="链路" value={(selectedConnection.chains || []).join(' → ')} />
                  <DetailRow label="进程" value={processName(selectedConnection)} />
                  <DetailRow label="进程路径" value={selectedConnection.metadata?.processPath} />
                  <DetailRow label="入站" value={selectedConnection.metadata?.inboundName || selectedConnection.metadata?.inboundUser || selectedConnection.metadata?.inboundIP} />
                </dl>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}
