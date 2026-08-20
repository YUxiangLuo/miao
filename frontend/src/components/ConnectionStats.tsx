import { ArrowDown, ArrowUp } from 'lucide-react'
import { ICON } from '../tokens'
import { formatBytes, formatSpeed } from '../utils'
import type { EnrichedConnection } from '../types/clash'
import { domainsForPath, splitConnectionStats, uniqueIconDomains, type PathStats } from './connectionFilters'
import { AnimatedValue, SiteMark } from './connectionUi'

/** favicon 条最多展示的图标数，超出折叠为 +N */
const MAX_ICONS = 8

function PathCard({ tone, label, stats, domains }: {
  tone: 'info' | 'success'
  label: string
  stats: PathStats
  /** 本通道去重域名榜（已按速率降序） */
  domains: string[]
}) {
  const unique = uniqueIconDomains(domains)
  const shown = unique.slice(0, MAX_ICONS)
  const overflow = unique.length - shown.length

  return (
    <div className="path-card">
      <div className="path-card-head">
        <span className={`badge ${tone}`}>{label}</span>
        <span className="path-count">{stats.count} 条链接</span>
      </div>
      <div className="path-speed">
        <small className="tone-download">
          <ArrowDown size={ICON.sm} />
          <AnimatedValue value={formatSpeed(stats.downloadSpeed)} />
        </small>
        <small className="tone-upload">
          <ArrowUp size={ICON.sm} />
          <AnimatedValue value={formatSpeed(stats.uploadSpeed)} />
        </small>
      </div>
      <div className="path-total">
        <span className="path-total-label">累计</span>
        <small className="tone-download">
          <ArrowDown size={ICON.xs} />
          <AnimatedValue value={formatBytes(stats.download)} />
        </small>
        <small className="tone-upload">
          <ArrowUp size={ICON.xs} />
          <AnimatedValue value={formatBytes(stats.upload)} />
        </small>
      </div>
      <div className="path-icons">
        {shown.map((domain) => (
          <SiteMark key={domain} domain={domain} small />
        ))}
        {overflow > 0 && <span className="path-icons-more">+{overflow}</span>}
      </div>
    </div>
  )
}

/** 直连 / 代理双通道统计卡：速率、累计流量、链接数与通道内站点 favicon 条 */
export function ConnectionStats({ connections }: { connections: EnrichedConnection[] }) {
  const stats = splitConnectionStats(connections)
  return (
    <div className="path-stats">
      <PathCard
        tone="info"
        label="代理"
        stats={stats.proxy}
        domains={domainsForPath(connections, false)}
      />
      <PathCard
        tone="success"
        label="直连"
        stats={stats.direct}
        domains={domainsForPath(connections, true)}
      />
    </div>
  )
}
