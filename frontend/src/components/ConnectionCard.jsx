import { ArrowDown, ArrowUp, ChevronDown } from 'lucide-react'
import { classNames, formatBytes, formatSpeed, formatUptime } from '../utils.js'
import { iconForDomain } from './siteIcons.js'
import { displayRuleText, groupSpeed } from './connectionFilters.js'

function connectionRule(connection) {
  const rule = connection.rule || '-'
  return connection.rulePayload ? `${rule} : ${connection.rulePayload}` : rule
}

// 轮询刷新时数值变化会触发 key 变化重新挂载,配合 CSS 淡入提示数据已更新
export function AnimatedValue({ value, className }) {
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

export function ConnectionCard({ group, expanded, onToggle }) {
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
