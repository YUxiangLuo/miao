import { ArrowDown, ArrowUp, ChevronDown } from 'lucide-react'
import { ICON } from '../tokens'
import { classNames, formatBytes, formatSpeed } from '../utils'
import type { GroupRow } from '../types/clash'
import { isDirectOutbound } from './connectionFilters'
import { AnimatedValue, ConnectionDetailRow, SiteMark } from './ConnectionCard'

export interface ConnectionRowProps {
  row: GroupRow
  /** 当前可见行的最大合计速率：比例条据此归一化 */
  maxSpeed: number
  expanded?: boolean
  onToggle?: () => void
}

/**
 * 链接统计的统一行（站点/进程/出口三维共用）。
 * 活跃行全亮并带速率比例条；空闲行降为 dim，把视觉权重让给正在传输的行。
 */
export function ConnectionRow({ row, maxSpeed, expanded, onToggle }: ConnectionRowProps) {
  const combined = row.downloadSpeed + row.uploadSpeed
  const active = combined > 0
  const ratio = maxSpeed > 0 ? Math.min(1, combined / maxSpeed) : 0
  // 比例条颜色 = 出口语义：直连绿 / 拦截红 / 代理路径蓝
  const barTone = isDirectOutbound(row.outbound) ? 'direct' : row.outbound === 'reject' ? 'reject' : 'proxy'

  return (
    <article className={classNames('conn-row', active ? 'active' : 'idle', expanded && 'expanded')}>
      <button
        type="button"
        className="conn-row-main"
        aria-expanded={expanded}
        aria-label={`${row.title} 链接详情`}
        onClick={onToggle}
      >
        <SiteMark domain={row.mark} />
        <div className="conn-row-id">
          <strong className="conn-row-title" title={row.title}>{row.title}</strong>
          <span className="conn-row-sub" title={row.subtitle}>{row.subtitle}</span>
        </div>
        <span className={classNames('badge', 'connection-outbound-chip', isDirectOutbound(row.outbound) && 'direct')}>
          {row.outbound}
        </span>
        {row.count > 1 && <span className="badge connection-count-chip">{row.count} 条链接</span>}
        <span className="conn-row-speed">
          <small className="tone-download">
            <ArrowDown size={ICON.xs} />
            <AnimatedValue value={formatSpeed(row.downloadSpeed)} />
          </small>
          <small className="tone-upload">
            <ArrowUp size={ICON.xs} />
            <AnimatedValue value={formatSpeed(row.uploadSpeed)} />
          </small>
          <i className={classNames('conn-speed-bar', `bar-${barTone}`)} aria-hidden="true">
            <i style={{ width: `${Math.round(ratio * 100)}%` }} />
          </i>
        </span>
        <span className="conn-row-total">
          <span className="connection-card-total-label">累计</span>
          <small className="tone-download">
            <ArrowDown size={ICON.xs} />
            <AnimatedValue value={formatBytes(row.download)} />
          </small>
          <small className="tone-upload">
            <ArrowUp size={ICON.xs} />
            <AnimatedValue value={formatBytes(row.upload)} />
          </small>
        </span>
        <ChevronDown size={ICON.sm} className="connection-card-chevron" aria-hidden="true" />
      </button>
      {expanded && (
        <div className="connection-card-details">
          {row.connections.map((item) => (
            <ConnectionDetailRow key={item.id} connection={item} />
          ))}
        </div>
      )}
    </article>
  )
}
