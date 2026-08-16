import { useId, useMemo, useState } from 'react'
import {
  Activity,
  ArrowDown,
  ArrowUp,
  Gauge,
  Globe,
  HardDriveDownload,
  X,
} from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { formatBytes, formatSpeed } from '../utils.js'
import { ConnectionsToolbar } from './ConnectionsToolbar.jsx'
import { AnimatedValue, ConnectionCard } from './ConnectionCard.jsx'
import {
  filterConnectionGroups,
  groupConnections,
  pathCountsFor,
  sortConnectionGroups,
} from './connectionFilters.js'

export function ConnectionsModal({
  open,
  status,
  data,
  loading,
  error,
  onClose,
}) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [query, setQuery] = useState('')
  const [path, setPath] = useState('all')
  const [sortKey, setSortKey] = useState('speed')
  const [expandedId, setExpandedId] = useState(null)

  const connections = useMemo(() => {
    return Array.isArray(data?.connections) ? data.connections : []
  }, [data?.connections])
  const uploadTotal = Number(data?.uploadTotal || connections.reduce((sum, item) => sum + Number(item.upload || 0), 0))
  const downloadTotal = Number(data?.downloadTotal || connections.reduce((sum, item) => sum + Number(item.download || 0), 0))
  const uploadSpeed = connections.reduce((sum, item) => sum + Number(item.uploadSpeed || 0), 0)
  const downloadSpeed = connections.reduce((sum, item) => sum + Number(item.downloadSpeed || 0), 0)

  const groups = useMemo(() => groupConnections(connections), [connections])
  const searchedGroups = useMemo(
    () => filterConnectionGroups(groups, { query, path: 'all' }),
    [groups, query],
  )
  const visibleGroups = useMemo(
    () => sortConnectionGroups(filterConnectionGroups(searchedGroups, { path }), sortKey),
    [path, searchedGroups, sortKey],
  )
  const pathCounts = useMemo(() => pathCountsFor(searchedGroups), [searchedGroups])

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
            <h3 id={titleId}>链接统计</h3>
            {status.running && (
              <span className="connections-live-badge">
                <i />
                {groups.length} 个站点 · {connections.length} 条链接
              </span>
            )}
          </div>
          <div className="connections-header-actions">
            <button className="icon-button" onClick={onClose} title="关闭 (Esc)" aria-label="关闭链接统计">
              <X size={16} />
            </button>
          </div>
        </header>

        {!status.running ? (
          <div className="connections-empty">服务未运行，暂无链接统计。</div>
        ) : (
          <div className="connections-body">
            <div className="connection-stat-grid">
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <Globe size={12} />
                  站点
                </span>
                <strong className="connection-stat-value">
                  <AnimatedValue value={groups.length} />
                </strong>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <Gauge size={12} />
                  实时速度
                </span>
                <strong className="connection-stat-value connection-stat-pair">
                  <span className="tone-download">
                    <ArrowDown size={13} />
                    <AnimatedValue value={formatSpeed(downloadSpeed)} />
                  </span>
                  <span className="tone-upload">
                    <ArrowUp size={13} />
                    <AnimatedValue value={formatSpeed(uploadSpeed)} />
                  </span>
                </strong>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <HardDriveDownload size={12} />
                  累计流量
                </span>
                <strong className="connection-stat-value connection-stat-pair">
                  <span className="tone-download">
                    <ArrowDown size={13} />
                    <AnimatedValue value={formatBytes(downloadTotal)} />
                  </span>
                  <span className="tone-upload">
                    <ArrowUp size={13} />
                    <AnimatedValue value={formatBytes(uploadTotal)} />
                  </span>
                </strong>
              </div>
            </div>

            {error && <div className="connections-error">{error}</div>}

            <div className="connections-main">
              <ConnectionsToolbar
                query={query}
                onQueryChange={setQuery}
                path={path}
                onPathChange={setPath}
                sortKey={sortKey}
                onSortChange={setSortKey}
                counts={pathCounts}
                resultCount={visibleGroups.length}
                totalCount={groups.length}
              />

              {visibleGroups.length > 0 ? (
                <div className="connection-card-grid">
                  {visibleGroups.map((group) => (
                    <ConnectionCard
                      key={group.id}
                      group={group}
                      expanded={expandedId === group.id}
                      onToggle={() => setExpandedId(expandedId === group.id ? null : group.id)}
                    />
                  ))}
                </div>
              ) : (
                <div className="connections-empty inline">
                  {loading && connections.length === 0 ? '加载中…' : '暂无匹配站点'}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
