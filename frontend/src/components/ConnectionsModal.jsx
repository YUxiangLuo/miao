import { useId, useMemo, useState } from 'react'
import {
  Activity,
  ArrowDown,
  ArrowUp,
  ChevronDown,
  Gauge,
  Globe,
  HardDriveDownload,
  RefreshCw,
  X,
} from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { classNames, formatBytes, formatSpeed, formatUptime } from '../utils.js'
import { connectionDomain, iconForDomain } from './siteIcons.js'
import { ConnectionsToolbar } from './ConnectionsToolbar.jsx'
import {
  displayRuleText,
  filterConnectionGroups,
  groupSpeed,
  pathCountsFor,
  sortConnectionGroups,
} from './connectionFilters.js'

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
      existing.download += Number(connection.download || 0)
      existing.upload += Number(connection.upload || 0)
      continue
    }

    groups.set(key, {
      id: key,
      domain,
      connections: [connection],
      downloadSpeed: Number(connection.downloadSpeed || 0),
      uploadSpeed: Number(connection.uploadSpeed || 0),
      download: Number(connection.download || 0),
      upload: Number(connection.upload || 0),
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
        ruleLabel: displayRuleText(rule.value),
        extraRules: rule.extra,
        outbound: outbound.value,
      }
    })
}

// 轮询刷新时数值变化会触发 key 变化重新挂载,配合 CSS 淡入提示数据已更新
function AnimatedValue({ value, className }) {
  return <span key={String(value)} className={classNames('value-anim', className)}>{value}</span>
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

function connectionDuration(connection) {
  const startedAt = Date.parse(connection.start || '')
  if (!Number.isFinite(startedAt)) return '--'
  return formatUptime(Math.max(0.1, (Date.now() - startedAt) / 1000))
}

function ConnectionDetailRow({ connection }) {
  const meta = connection.metadata || {}
  const chain = Array.isArray(connection.chains) && connection.chains.length > 0
    ? connection.chains.join(' → ')
    : 'direct'
  const network = [
    meta.network ? String(meta.network).toUpperCase() : '',
    meta.destinationPort || '',
  ].filter(Boolean).join('/')
  const ruleText = displayRuleText(connectionRule(connection))

  return (
    <div className="connection-detail-row">
      <div className="connection-detail-line">
        <span className="connection-detail-rule" title={ruleText}>{ruleText}</span>
        <span
          className="connection-detail-duration"
          title={connection.start ? `建立于 ${connection.start}` : undefined}
        >
          {connectionDuration(connection)}
        </span>
      </div>
      <div className="connection-detail-line">
        {network && <span className="connection-detail-net">{network}</span>}
        <span className="connection-detail-chain" title={chain}>{chain}</span>
        <span className="connection-detail-bytes">
          <small className="tone-download">
            <ArrowDown size={11} />
            {formatBytes(connection.download)}
          </small>
          <small className="tone-upload">
            <ArrowUp size={11} />
            {formatBytes(connection.upload)}
          </small>
        </span>
      </div>
    </div>
  )
}

function ConnectionCard({ group, expanded, onToggle }) {
  const active = groupSpeed(group) > 0
  return (
    <article className={classNames('connection-card', active && 'active', expanded && 'expanded')}>
      <button
        type="button"
        className="connection-card-head"
        aria-expanded={expanded}
        aria-label={`${group.domain} 链接详情`}
        onClick={onToggle}
      >
        <div className="connection-card-top">
          <SiteMark domain={group.domain} />
          <div className="connection-card-identity">
            <strong className="connection-card-domain" title={group.domain}>{group.domain}</strong>
            <span className="connection-card-rule" title={group.rule}>
              {group.ruleLabel}
              {group.extraRules > 0 && <em>+{group.extraRules}</em>}
            </span>
          </div>
          {active && <span className="connection-live-dot" title="正在传输" aria-hidden="true" />}
          <ChevronDown size={14} className="connection-card-chevron" aria-hidden="true" />
        </div>
        <div className="connection-card-meta">
          <span className={classNames('connection-outbound-chip', group.outbound === 'direct' && 'direct')}>
            {group.outbound}
          </span>
          {group.count > 1 && <span className="connection-count-chip">{group.count} 条链接</span>}
          <span className="connection-card-speed">
            <small className="tone-download">
              <ArrowDown size={11} />
              <AnimatedValue value={formatSpeed(group.downloadSpeed)} />
            </small>
            <small className="tone-upload">
              <ArrowUp size={11} />
              <AnimatedValue value={formatSpeed(group.uploadSpeed)} />
            </small>
          </span>
        </div>
        <div className="connection-card-total">
          <span className="connection-card-total-label">累计</span>
          <span className="connection-card-total-values">
            <small className="tone-download">
              <ArrowDown size={11} />
              <AnimatedValue value={formatBytes(group.download)} />
            </small>
            <small className="tone-upload">
              <ArrowUp size={11} />
              <AnimatedValue value={formatBytes(group.upload)} />
            </small>
          </span>
        </div>
      </button>
      {expanded && (
        <div className="connection-card-details">
          {group.connections.map((item) => (
            <ConnectionDetailRow key={item.id} connection={item} />
          ))}
        </div>
      )}
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
            <button className="connections-tool-button" onClick={onRefresh} disabled={loading || !status.running}>
              <RefreshCw size={14} className={loading ? 'spin' : undefined} />
              刷新
            </button>
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
