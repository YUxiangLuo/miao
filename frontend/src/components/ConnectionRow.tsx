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
}

/**
 * 链接统计的单条链接行：域名为主标识，副行是人话规则 · 进程 · 网络/端口 · 时长；
 * 活跃链接全亮并带速率比例条，空闲链接降为 dim。
 */
export function ConnectionRow({ connection }: ConnectionRowProps) {
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
  // 双段比例条：蓝段=下载、绿段=上传（与正上方两组数字同色）。
  // 每行恒满宽：段宽 = 各自 ÷ 本行合计——表达上下行构成而非绝对量
  // （量级由数字与排序承担；按全场面最大值归一会让大多数行常年只见一截）
  const downShare = combined > 0 ? connection.downloadSpeed / combined : 0
  const upShare = combined > 0 ? connection.uploadSpeed / combined : 0

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
          <i className="conn-speed-bar" aria-hidden="true">
            <i className="seg-down" style={{ width: `${Math.round(downShare * 100)}%` }} />
            <i className="seg-up" style={{ width: `${Math.round(upShare * 100)}%` }} />
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
