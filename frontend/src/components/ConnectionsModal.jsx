import { useId, useMemo, useState } from 'react'
import { Activity, ArrowDown, ArrowUp, Network, RefreshCw, X } from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { classNames, formatBytes, formatSpeed } from '../utils.js'
import { connectionDomain, iconForDomain } from './siteIcons.js'
import { ConnectionsToolbar } from './ConnectionsToolbar.jsx'
import { filterConnectionGroups, pathCountsFor, sortConnectionGroups } from './connectionFilters.js'

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

function majorityValue(items, mapper) {
  const counts = new Map()
  for (const item of items) {
    const value = mapper(item) || '-'
    counts.set(value, (counts.get(value) || 0) + 1)
  }

  let winner = '-'
  let winnerCount = 0
  for (const [value, count] of counts) {
    if (count > winnerCount) {
      winner = value
      winnerCount = count
    }
  }

  return { value: winner, extra: Math.max(0, counts.size - 1) }
}

function groupConnections(connections) {
  const groups = new Map()

  for (const connection of connections) {
    const domain = connectionDomain(connection)
    const key = domain.toLowerCase()
    const existing = groups.get(key)
    if (existing) {
      existing.connections.push(connection)
      existing.downloadSpeed += Number(connection.downloadSpeed || 0)
      existing.uploadSpeed += Number(connection.uploadSpeed || 0)
      continue
    }

    groups.set(key, {
      domain,
      connections: [connection],
      downloadSpeed: Number(connection.downloadSpeed || 0),
      uploadSpeed: Number(connection.uploadSpeed || 0),
    })
  }

  return [...groups.values()]
    .map((group) => {
      const rule = majorityValue(group.connections, connectionRule)
      const outbound = majorityValue(group.connections, connectionOutbound)
      return {
        ...group,
        count: group.connections.length,
        rule: rule.value,
        extraRules: rule.extra,
        outbound: outbound.value,
      }
    })
}

function SiteMark({ domain }) {
  const icon = iconForDomain(domain)
  return (
    <div
      className={classNames('site-icon', icon.id === 'letter' && 'letter')}
      style={icon.path ? { background: icon.background, color: icon.color } : undefined}
      data-site={icon.id}
      title={icon.label}
      aria-hidden="true"
    >
      {icon.path ? (
        <svg viewBox={icon.viewBox || '0 0 24 24'} width="18" height="18">
          <path fill="currentColor" d={icon.path} />
        </svg>
      ) : (
        <span>{icon.letter}</span>
      )}
    </div>
  )
}

function ConnectionCard({ group, closing, onClose }) {
  return (
    <article className="connection-card">
      <button
        type="button"
        className="connection-card-close"
        onClick={() => onClose(group)}
        disabled={closing}
        title="关闭连接"
        aria-label={`关闭 ${group.domain} 的连接`}
      >
        {closing ? <RefreshCw size={13} className="spin" /> : <X size={13} />}
      </button>
      <div className="connection-card-top">
        <SiteMark domain={group.domain} />
        <div className="connection-card-identity">
          <strong className="connection-card-domain" title={group.domain}>{group.domain}</strong>
          <span className="connection-card-rule" title={group.rule}>
            {group.rule}
            {group.extraRules > 0 && <em>+{group.extraRules}</em>}
          </span>
        </div>
      </div>
      <div className="connection-card-meta">
        <span className={classNames('connection-outbound-chip', group.outbound === 'direct' && 'direct')}>
          {group.outbound}
        </span>
        {group.count > 1 && <span className="connection-count-chip">{group.count} 条</span>}
        <span className="connection-card-speed">
          <small className="tone-download"><ArrowDown size={11} />{formatSpeed(group.downloadSpeed)}</small>
          <small className="tone-upload"><ArrowUp size={11} />{formatSpeed(group.uploadSpeed)}</small>
        </span>
      </div>
    </article>
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
  const [path, setPath] = useState('all')
  const [sortKey, setSortKey] = useState('activity')
  const [closingDomain, setClosingDomain] = useState('')

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

  const handleCloseGroup = async (group) => {
    setClosingDomain(group.domain)
    try {
      for (const connection of group.connections) {
        if (connection.id) await onCloseConnection(connection.id)
      }
    } catch (closeError) {
      showToast?.(closeError.message || '关闭连接失败', 'error')
    } finally {
      setClosingDomain('')
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
                <strong className="connection-stat-value tone-download">{formatSpeed(downloadSpeed)}</strong>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <ArrowUp size={12} />
                  上传速度
                </span>
                <strong className="connection-stat-value tone-upload">{formatSpeed(uploadSpeed)}</strong>
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
                      key={group.domain}
                      group={group}
                      closing={closingDomain === group.domain}
                      onClose={handleCloseGroup}
                    />
                  ))}
                </div>
              ) : (
                <div className="connections-empty inline">
                  {loading && connections.length === 0 ? '加载中…' : '暂无匹配连接'}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
