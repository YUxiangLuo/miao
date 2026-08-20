import { useMemo, useRef } from 'react'
import type { CSSProperties } from 'react'
import { Activity } from 'lucide-react'
import { ICON, STAGGER_CAP } from '../tokens'
import { classNames } from '../utils'
import { iconForDomain, mainDomain } from './siteIcons'
import { groupConnections, groupSpeed, sortConnectionGroups, isDirectOutbound } from './connectionFilters'
import type { StatusData } from '../types/api'
import type { ConnectionGroup, ConnectionsInfo } from '../types/clash'

// 首页条带专用卡：圆角正方形，favicon 为主视觉 + 主域名 + 出口 chip。
// 首页条带是独立的轻量呈现（图标+主域名+出口），与链接统计面板的行式列表各自演进
// 首页只需要「谁在用网、走的哪条出口」的一眼概览；明细由「查看全部」承载。
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

  // 条带顺序 = 首次出现先后，新组永远最前：每轮把未见过的 id（按传入的
  // 速率降序）前插到既有顺序之前，既有组保持原位（不随速率波动重排）；
  // 消失的组从顺序册移除，再出现时按全新处理（回最前）。
  // ref 变更幂等，useMemo/StrictMode 重放安全。
  const orderRef = useRef<string[]>([])
  const orderedGroups = useMemo(() => {
    const byId = new Map(activeGroups.map((group) => [group.id, group]))
    const prevSet = new Set(orderRef.current)
    const fresh = activeGroups.map((group) => group.id).filter((id) => !prevSet.has(id))
    const kept = orderRef.current.filter((id) => byId.has(id))
    orderRef.current = [...fresh, ...kept]
    return orderRef.current.map((id) => byId.get(id)!)
  }, [activeGroups])

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
          orderedGroups.map((group, index) => (
            <HomeSiteCard key={group.id} group={group} index={index} />
          ))
        ) : (
          <div className="empty-block home-connections-empty">暂无活跃链接</div>
        )}
      </div>
    </section>
  )
}
