import { useEffect, useId, useMemo, useState } from 'react'
import { Activity, ArrowDown, ArrowUp, Network, RefreshCw, Route, Search, X } from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { classNames, formatBytes, formatSpeed } from '../utils.js'

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

const SPEED_HISTORY_LIMIT = 40

function SpeedSparkline({ points, tone }) {
  const width = 140
  const height = 34
  if (!points || points.length < 2) {
    return <div className="connection-sparkline placeholder" aria-hidden="true" />
  }
  const max = Math.max(...points, 1)
  const step = width / Math.max(points.length - 1, 1)
  const coords = points.map((value, index) => {
    const x = (index * step).toFixed(1)
    const y = (height - 3 - (value / max) * (height - 8)).toFixed(1)
    return `${x},${y}`
  })
  return (
    <svg
      className={classNames('connection-sparkline', tone)}
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <polygon className="spark-fill" points={`0,${height} ${coords.join(' ')} ${width},${height}`} />
      <polyline className="spark-line" points={coords.join(' ')} />
    </svg>
  )
}

function DistributionPanel({ icon, title, counts, total }) {
  return (
    <div className="connections-panel">
      <div className="connections-panel-title">
        {icon}
        <span>{title}</span>
      </div>
      <div className="connections-panel-body">
        {counts.length > 0 ? counts.map(([name, count]) => {
          const percent = total > 0 ? Math.max((count / total) * 100, 3) : 0
          return (
            <div className="dist-row" key={name}>
              <div className="dist-row-info">
                <span title={name}>{name}</span>
                <strong>{count}</strong>
              </div>
              <div className="dist-bar">
                <i style={{ width: `${percent}%` }} />
              </div>
            </div>
          )
        }) : <div className="connections-muted">暂无数据</div>}
      </div>
    </div>
  )
}

function DetailItem({ label, value }) {
  return (
    <div className="connection-detail-item">
      <span>{label}</span>
      <strong title={String(value || '-')}>{value || '-'}</strong>
    </div>
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
  showToast,
}) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [query, setQuery] = useState('')
  const [sourceFilter, setSourceFilter] = useState('')
  const [sortKey, setSortKey] = useState('downloadSpeed')
  const [sortDesc, setSortDesc] = useState(true)
  const [page, setPage] = useState(0)
  const [selectedId, setSelectedId] = useState('')
  const [closingId, setClosingId] = useState('')
  const [speedHistory, setSpeedHistory] = useState({ down: [], up: [] })

  useEffect(() => {
    if (open) {
      setPage(0)
      setSpeedHistory({ down: [], up: [] })
    }
  }, [open])

  useEffect(() => {
    setPage(0)
  }, [query, sourceFilter, sortKey, sortDesc])

  const connections = useMemo(() => {
    return Array.isArray(data?.connections) ? data.connections : []
  }, [data?.connections])
  const uploadTotal = Number(data?.uploadTotal || connections.reduce((sum, item) => sum + Number(item.upload || 0), 0))
  const downloadTotal = Number(data?.downloadTotal || connections.reduce((sum, item) => sum + Number(item.download || 0), 0))
  const uploadSpeed = connections.reduce((sum, item) => sum + Number(item.uploadSpeed || 0), 0)
  const downloadSpeed = connections.reduce((sum, item) => sum + Number(item.downloadSpeed || 0), 0)

  // 每轮连接数据采样一次整体速度，用于统计卡片中的趋势图
  useEffect(() => {
    if (!open || !status.running) return
    setSpeedHistory((prev) => ({
      down: [...prev.down, downloadSpeed].slice(-SPEED_HISTORY_LIMIT),
      up: [...prev.up, uploadSpeed].slice(-SPEED_HISTORY_LIMIT),
    }))
  }, [data]) // eslint-disable-line react-hooks/exhaustive-deps

  const networkCounts = topEntries(countBy(connections, (item) => item.metadata?.network), 6)
  const outboundCounts = topEntries(countBy(connections, connectionOutbound), 8)
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

  if (!open) return null

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card connections-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="connections-header">
          <div className="connections-header-title">
            <Activity size={18} className="icon-accent" />
            <h3 id={titleId}>连接统计</h3>
            {status.running && (
              <span className="connections-live-badge">
                <i />
                {connections.length} 个活跃连接
              </span>
            )}
          </div>
          <div className="connections-header-actions">
            <button className="connections-tool-button" onClick={onRefresh} disabled={loading || !status.running}>
              <RefreshCw size={14} className={loading ? 'spin' : undefined} />
              刷新
            </button>
            <button className="icon-button" onClick={onClose} title="关闭 (Esc)" aria-label="关闭连接统计">
              <X size={16} />
            </button>
          </div>
        </header>

        {!status.running ? (
          <div className="connections-empty">服务未运行，暂无连接统计。</div>
        ) : (
          <div className="connections-body">
            <div className="connection-stat-grid">
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <Network size={12} />
                  活跃连接
                </span>
                <strong className="connection-stat-value">{connections.length}</strong>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <ArrowDown size={12} />
                  下载速度
                </span>
                <div className="connection-stat-main">
                  <strong className="connection-stat-value tone-download">{formatSpeed(downloadSpeed)}</strong>
                  <SpeedSparkline points={speedHistory.down} tone="down" />
                </div>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <ArrowUp size={12} />
                  上传速度
                </span>
                <div className="connection-stat-main">
                  <strong className="connection-stat-value tone-upload">{formatSpeed(uploadSpeed)}</strong>
                  <SpeedSparkline points={speedHistory.up} tone="up" />
                </div>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <ArrowDown size={12} />
                  累计下载
                </span>
                <strong className="connection-stat-value">{formatBytes(downloadTotal)}</strong>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <ArrowUp size={12} />
                  累计上传
                </span>
                <strong className="connection-stat-value">{formatBytes(uploadTotal)}</strong>
              </div>
            </div>

            {error && <div className="connections-error">{error}</div>}

            <div className="connections-content">
              <aside className="connections-sidebar">
                <DistributionPanel icon={<Network size={13} />} title="协议分布" counts={networkCounts} total={connections.length} />
                <DistributionPanel icon={<Route size={13} />} title="出口分布" counts={outboundCounts} total={connections.length} />
              </aside>

              <div className="connections-main">
                <div className="connections-toolbar">
                  <label className="connections-search">
                    <Search size={14} />
                    <input
                      data-autofocus
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
                  <button className="connections-tool-button" onClick={() => setSortDesc((value) => !value)} title="切换排序方向">
                    {sortDesc ? '降序' : '升序'}
                  </button>
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
                    >
                      <button
                        type="button"
                        className="connection-row-close"
                        onClick={() => handleCloseSingle(connection.id)}
                        disabled={closingId === connection.id}
                        title="关闭连接"
                        aria-label={`关闭连接 ${connectionTarget(connection)}`}
                      >
                        {closingId === connection.id ? <RefreshCw size={13} className="spin" /> : <X size={13} />}
                      </button>
                      <button
                        type="button"
                        className="connection-row-summary"
                        aria-label={`查看连接 ${connectionTarget(connection)} 的详情`}
                        aria-expanded={selectedId === connection.id}
                        onClick={() => setSelectedId(connection.id)}
                      >
                        <span className="connection-host" title={connectionTarget(connection)}>
                          <strong>{connectionTarget(connection)}</strong>
                          <small>
                            <em className="connection-network-badge">{connection.metadata?.network || '-'}</em>
                            {processName(connection)} · {formatStartTime(connection.start)}
                          </small>
                        </span>
                        <span className="connection-rule" title={`${connectionRule(connection)} → ${(connection.chains || []).join(' → ')}`}>
                          <strong>{connectionRule(connection)}</strong>
                          <small>{(connection.chains || []).length ? [...connection.chains].reverse().join(' → ') : connectionOutbound(connection)}</small>
                        </span>
                        <span title={connectionSource(connection)}>{connectionSource(connection)}</span>
                        <span className="connection-speed">
                          <small className="tone-download"><ArrowDown size={12} />{formatSpeed(Number(connection.downloadSpeed || 0))}</small>
                          <small className="tone-upload"><ArrowUp size={12} />{formatSpeed(Number(connection.uploadSpeed || 0))}</small>
                        </span>
                        <span>
                          <small><ArrowDown size={12} />{formatBytes(Number(connection.download || 0))}</small>
                          <small><ArrowUp size={12} />{formatBytes(Number(connection.upload || 0))}</small>
                        </span>
                      </button>
                    </div>
                  )) : (
                    <div className="connections-empty inline">
                      {loading && connections.length === 0 ? '加载中…' : '暂无匹配连接'}
                    </div>
                  )}
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
              </div>

              {selectedConnection && (
                <aside className="connection-detail-side">
                  <div className="connection-detail-title">
                    <strong>连接详情</strong>
                    <button className="icon-button subtle" onClick={() => setSelectedId('')} title="关闭详情">
                      <X size={14} />
                    </button>
                  </div>
                  <div className="connection-detail-items">
                    <DetailItem label="目标" value={connectionTarget(selectedConnection)} />
                    <DetailItem label="远端目标" value={connectionDestination(selectedConnection)} />
                    <DetailItem label="来源" value={connectionSource(selectedConnection)} />
                    <DetailItem label="规则" value={connectionRule(selectedConnection)} />
                    <DetailItem label="链路" value={(selectedConnection.chains || []).join(' → ')} />
                    <DetailItem label="网络" value={`${selectedConnection.metadata?.type || '-'} / ${selectedConnection.metadata?.network || '-'}`} />
                    <DetailItem label="进程" value={processName(selectedConnection)} />
                    <DetailItem label="进程路径" value={selectedConnection.metadata?.processPath} />
                    <DetailItem label="开始时间" value={formatStartTime(selectedConnection.start)} />
                    <DetailItem label="入站" value={selectedConnection.metadata?.inboundName || selectedConnection.metadata?.inboundUser || selectedConnection.metadata?.inboundIP} />
                    <DetailItem label="ID" value={selectedConnection.id} />
                  </div>
                  <div className="connection-detail-actions">
                    <button
                      className="connections-tool-button danger block"
                      onClick={() => handleCloseSingle(selectedConnection.id)}
                      disabled={closingId === selectedConnection.id}
                    >
                      {closingId === selectedConnection.id ? <RefreshCw size={14} className="spin" /> : <X size={14} />}
                      关闭此连接
                    </button>
                  </div>
                </aside>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
