import { memo, useMemo } from 'react'
import { ArrowDown, ArrowUp } from 'lucide-react'
import { ICON, PATH_ICON_CELL } from '../tokens'
import { formatBytes, formatSpeed } from '../utils'
import { useElementWidth } from '../hooks/useElementWidth'
import type { EnrichedConnection } from '../types/clash'
import { domainsForPath, splitConnectionStats, uniqueIconDomains, type PathStats } from './connectionFilters'
import { AnimatedValue, SiteMark } from './connectionUi'
import { foldIcons, MAX_ICONS } from './iconFold'

function PathCard({ tone, label, stats, domains }: {
  tone: 'info' | 'success'
  label: string
  stats: PathStats
  /** 本通道去重域名榜（已按速率降序） */
  domains: string[]
}) {
  const unique = uniqueIconDomains(domains)
  // 图标尺寸恒定，单行放不下才折 +N：实测图标区宽度决定单行容量
  const { ref, width } = useElementWidth<HTMLDivElement>()
  const fit = Number.isFinite(width) ? Math.max(1, Math.floor(width / PATH_ICON_CELL)) : MAX_ICONS
  const plan = foldIcons(unique.length, fit)
  const shown = unique.slice(0, plan.shown)

  return (
    <div className="path-card">
      <div className="path-id">
        <span className={`badge ${tone}`}>{label}</span>
        <span className="path-count">{stats.count} 条链接</span>
      </div>
      {/* 速率列：上下行堆叠，/s 单位自说明，无需标签 */}
      <div className="path-col speed">
        <small className="tone-download">
          <ArrowDown size={ICON.sm} />
          <AnimatedValue value={formatSpeed(stats.downloadSpeed)} />
        </small>
        <small className="tone-upload">
          <ArrowUp size={ICON.sm} />
          <AnimatedValue value={formatSpeed(stats.uploadSpeed)} />
        </small>
      </div>
      {/* 累计列：与速率列平行的上下行（小字号次要指标，裸字节数即累计量） */}
      <div className="path-col total">
        <small className="tone-download">
          <ArrowDown size={ICON.xs} />
          <AnimatedValue value={formatBytes(stats.download)} />
        </small>
        <small className="tone-upload">
          <ArrowUp size={ICON.xs} />
          <AnimatedValue value={formatBytes(stats.upload)} />
        </small>
      </div>
      <div ref={ref} className="path-icons">
        {shown.map((domain) => (
          <SiteMark key={domain} domain={domain} />
        ))}
        {plan.more > 0 && <span className="path-icons-more">+{plan.more}</span>}
      </div>
    </div>
  )
}

/** 直连 / 代理双通道统计卡：速率、累计流量、链接数与通道内站点 favicon 条 */
export const ConnectionStats = memo(function ConnectionStats({ connections }: { connections: EnrichedConnection[] }) {
  const stats = useMemo(() => splitConnectionStats(connections), [connections])
  const proxyDomains = useMemo(() => domainsForPath(connections, false), [connections])
  const directDomains = useMemo(() => domainsForPath(connections, true), [connections])
  return (
    <div className="path-stats">
      <PathCard
        tone="info"
        label="代理"
        stats={stats.proxy}
        domains={proxyDomains}
      />
      <PathCard
        tone="success"
        label="直连"
        stats={stats.direct}
        domains={directDomains}
      />
    </div>
  )
})
