import { useMemo, useRef } from 'react'
import type { CSSProperties } from 'react'
import { Activity } from 'lucide-react'
import { ICON, STAGGER_CAP } from '../tokens'
import { useElementWidth } from '../hooks/useElementWidth'
import { classNames } from '../utils'
import { iconForDomain, mainDomain } from './siteIcons'
import { groupConnections, groupSpeed, sortConnectionGroups, isDirectOutbound } from './connectionFilters'
import type { StatusData } from '../types/api'
import type { ConnectionGroup, ConnectionsInfo } from '../types/clash'
import { foldHomeConnections, mergeActiveGroupOrder } from './homeConnectionsFold'

// 首页条带专用卡：圆角正方形，favicon 为主视觉 + 主域名 + 出口 chip。
// 首页条带是独立的轻量呈现（图标+主域名+出口），与链接统计面板的行式列表各自演进
// 首页只需要「谁在用网、走的哪条出口」的一眼概览；溢出时由 +N 进入明细。
function HomeSiteCard({ group, index = 0 }: { group: ConnectionGroup; index?: number }) {
  const main = mainDomain(group.domain)
  const brandIcon = iconForDomain(group.domain)
  // 品牌匹配用完整域名（子域命中更准）；无品牌时字母 fallback 取主域名首字母
  const icon = brandIcon.id === 'letter' ? iconForDomain(main) : brandIcon
  const isLetter = 'letter' in icon
  const direct = isDirectOutbound(group.outbound)
  return (
    <article
      className="home-site-card"
      style={{ '--i': Math.min(index, STAGGER_CAP) } as CSSProperties}
      title={`${group.domain} → ${group.outbound}`}
    >
      <div
        className={classNames('home-site-icon', isLetter && 'letter')}
        style={!isLetter ? { background: icon.background, color: icon.color } : undefined}
        aria-hidden="true"
      >
        {!isLetter ? (
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

export interface HomeConnectionsProps {
  status: StatusData
  data: ConnectionsInfo
  onOpenAll: () => void
}

export function HomeConnections({ status, data, onOpenAll }: HomeConnectionsProps) {
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

  // ref 变更幂等，useMemo/StrictMode 重放安全。
  const orderRef = useRef<string[]>([])
  const orderedGroups = useMemo(() => {
    const merged = mergeActiveGroupOrder(orderRef.current, activeGroups)
    orderRef.current = merged.nextOrder
    return merged.orderedGroups
  }, [activeGroups])

  const hasActive = Boolean(status.ready) && activeGroups.length > 0
  const { ref: gridRef, width: gridWidth } = useElementWidth<HTMLDivElement>()
  const foldPlan = foldHomeConnections(orderedGroups.length, gridWidth)
  const shownGroups = orderedGroups.slice(0, foldPlan.shown)

  // 始终渲染占位:不出现时不渲染会让 .content-grid 变成 last-child 而撑高,
  // 导致活跃链接出现时主内容区高度突变、右列突然出现滚动条。
  // 条带恒高、正方形卡片单行排开，放不下的折叠为 +N，由 +N 进入明细。
  return (
    <section className="home-connections" aria-label="活跃链接">
      <div className="home-connections-header">
        <div className="home-connections-title">
          <Activity size={ICON.sm} className="section-icon" />
          <span>活跃链接</span>
        </div>
      </div>
      <div ref={gridRef} className="home-connections-grid">
        {hasActive ? (
          <>
            {shownGroups.map((group, index) => (
              <HomeSiteCard key={group.id} group={group} index={index} />
            ))}
            {foldPlan.more > 0 && (
              <button
                type="button"
                className="home-connections-more"
                aria-label={`打开其余 ${foldPlan.more} 个活跃链接`}
                onClick={onOpenAll}
              >
                +{foldPlan.more}
              </button>
            )}
          </>
        ) : (
          <div className="empty-block home-connections-empty">暂无活跃链接</div>
        )}
      </div>
    </section>
  )
}
