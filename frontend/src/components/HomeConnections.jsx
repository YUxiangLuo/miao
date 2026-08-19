import { useMemo } from 'react'
import { Activity } from 'lucide-react'
import { ICON } from '../tokens.js'
import { classNames } from '../utils.js'
import { iconForDomain, mainDomain } from './siteIcons.js'
import { groupConnections, groupSpeed, sortConnectionGroups, isDirectOutbound } from './connectionFilters.js'

// 首页条带专用卡：圆角正方形，favicon 为主视觉 + 主域名 + 出口 chip。
// 刻意不复用链接统计面板的 ConnectionCard——那里承载速率/累计/展开明细，
// 首页只需要「谁在用网、走的哪条出口」的一眼概览；明细由「查看全部」承载。
function HomeSiteCard({ group }) {
  const main = mainDomain(group.domain)
  const brandIcon = iconForDomain(group.domain)
  // 品牌匹配用完整域名（子域命中更准）；无品牌时字母 fallback 取主域名首字母
  const icon = brandIcon.id === 'letter' ? iconForDomain(main) : brandIcon
  const direct = isDirectOutbound(group.outbound)
  return (
    <article className="home-site-card" title={`${group.domain} → ${group.outbound}`}>
      <div
        className={classNames('home-site-icon', icon.id === 'letter' && 'letter')}
        style={icon.path ? { background: icon.background, color: icon.color } : undefined}
        aria-hidden="true"
      >
        {icon.path ? (
          <svg viewBox={icon.viewBox || '0 0 24 24'} width="36" height="36">
            <path fill="currentColor" d={icon.path} />
          </svg>
        ) : (
          <span>{icon.letter}</span>
        )}
      </div>
      <span className="home-site-domain">{main}</span>
      <span className={classNames('badge', 'home-site-outbound', direct ? 'success' : 'info')}>
        {group.outbound}
      </span>
    </article>
  )
}

export function HomeConnections({ status, data, onOpenAll }) {
  const connections = useMemo(
    () => (Array.isArray(data?.connections) ? data.connections : []),
    [data?.connections],
  )
  const activeGroups = useMemo(
    () => sortConnectionGroups(
      groupConnections(connections).filter((group) => groupSpeed(group) > 0),
    ),
    [connections],
  )

  const hasActive = status.running && activeGroups.length > 0

  // 始终渲染占位:不出现时不渲染会让 .content-grid 变成 last-child 而撑高,
  // 导致活跃链接出现时主内容区高度突变、右列突然出现滚动条。
  // 条带恒高、正方形卡片单行排开,放不下的直接裁掉,明细走「查看全部」。
  return (
    <section className="home-connections" aria-label="活跃链接">
      <div className="home-connections-header">
        <div className="home-connections-title">
          <Activity size={ICON.sm} className="section-icon" />
          <span>活跃链接</span>
          <span className="badge home-connections-count">{activeGroups.length}</span>
        </div>
        <button type="button" className="home-connections-all" onClick={onOpenAll}>
          查看全部
        </button>
      </div>
      <div className="home-connections-grid">
        {hasActive ? (
          activeGroups.map((group) => <HomeSiteCard key={group.id} group={group} />)
        ) : (
          <div className="empty-block home-connections-empty">暂无活跃链接</div>
        )}
      </div>
    </section>
  )
}
