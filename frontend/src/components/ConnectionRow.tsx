import { ArrowDown, ArrowUp } from 'lucide-react'
import { ICON } from '../tokens'
import { classNames, formatBytes, formatSpeed } from '../utils'
import type { EnrichedConnection } from '../types/clash'
import {
  connectionOutbound,
  connectionRule,
  connectionSpeed,
  connectionDuration,
  humanizeClashRule,
  displayRuleText,
  isDirectOutbound,
  processNameOf,
} from './connectionFilters'
import { connectionDomain } from './siteIcons'
import { AnimatedValue, SiteMark } from './ConnectionCard'

export interface ConnectionRowProps {
  connection: EnrichedConnection
  /** 当前可见链接的最大合计速率：比例条据此归一化 */
  maxSpeed: number
}

/**
 * 链接统计的单条链接行：域名为主标识，副行是人话规则 · 进程 · 网络/端口 · 时长；
 * 活跃链接全亮并带速率比例条，空闲链接降为 dim。
 */
export function ConnectionRow({ connection, maxSpeed }: ConnectionRowProps) {
  const domain = connectionDomain(connection)
  const outbound = connectionOutbound(connection)
  const meta = connection.metadata || {}
  const ruleText = humanizeClashRule(connectionRule(connection)) || displayRuleText(connectionRule(connection))
  const processName = processNameOf(meta.processPath)
  const network = [
    meta.network ? String(meta.network).toUpperCase() : '',
    meta.destinationPort || '',
  ].filter(Boolean).join('/')
  const duration = connectionDuration(connection)
  const subtitle = [ruleText, processName, network, duration !== '--' ? duration : null]
    .filter(Boolean)
    .join(' · ')

  const combined = connectionSpeed(connection)
  const active = combined > 0
  const ratio = maxSpeed > 0 ? Math.min(1, combined / maxSpeed) : 0
  // 比例条颜色 = 出口语义：直连绿 / 拦截红 / 代理路径蓝
  const barTone = isDirectOutbound(outbound) ? 'direct' : outbound === 'reject' ? 'reject' : 'proxy'

  return (
    <article
      className={classNames('conn-row', active ? 'active' : 'idle')}
      aria-label={`${domain} 链接`}
    >
      <div className="conn-row-main">
        <SiteMark domain={domain} />
        <div className="conn-row-id">
          <strong className="conn-row-title" title={domain}>{domain}</strong>
          <span className="conn-row-sub" title={subtitle}>{subtitle}</span>
        </div>
        <span className={classNames('badge', 'connection-outbound-chip', isDirectOutbound(outbound) && 'direct')}>
          {outbound}
        </span>
        <span className="conn-row-speed">
          <small className="tone-download">
            <ArrowDown size={ICON.xs} />
            <AnimatedValue value={formatSpeed(connection.downloadSpeed)} />
          </small>
          <small className="tone-upload">
            <ArrowUp size={ICON.xs} />
            <AnimatedValue value={formatSpeed(connection.uploadSpeed)} />
          </small>
          <i className={classNames('conn-speed-bar', `bar-${barTone}`)} aria-hidden="true">
            <i style={{ width: `${Math.round(ratio * 100)}%` }} />
          </i>
        </span>
        <span className="conn-row-total">
          <span className="connection-card-total-label">累计</span>
          <small className="tone-download">
            <ArrowDown size={ICON.xs} />
            <AnimatedValue value={formatBytes(connection.download)} />
          </small>
          <small className="tone-upload">
            <ArrowUp size={ICON.xs} />
            <AnimatedValue value={formatBytes(connection.upload)} />
          </small>
        </span>
      </div>
    </article>
  )
}
