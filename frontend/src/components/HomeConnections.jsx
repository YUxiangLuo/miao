import { useMemo } from 'react'
import { Activity } from 'lucide-react'
import { ConnectionCard } from './ConnectionCard.jsx'
import { groupConnections, groupSpeed, sortConnectionGroups } from './connectionFilters.js'

export function HomeConnections({ status, data, onOpenAll }) {
  const connections = useMemo(
    () => (Array.isArray(data?.connections) ? data.connections : []),
    [data?.connections],
  )
  const activeGroups = useMemo(
    () => sortConnectionGroups(
      groupConnections(connections).filter((group) => groupSpeed(group) > 0),
      'speed',
    ),
    [connections],
  )

  const hasActive = status.running && activeGroups.length > 0

  // 始终渲染占位:不出现时不渲染会让 .content-grid 变成 last-child 而撑高,
  // 导致活跃链接出现时主内容区高度突变、右列突然出现滚动条。
  // 条带恒高、只显示一行,放不下的卡片直接裁掉,明细走「查看全部」。
  return (
    <section className="home-connections" aria-label="活跃链接">
      <div className="home-connections-header">
        <div className="home-connections-title">
          <Activity size={14} className="section-icon" />
          <span>活跃链接</span>
          <span className="home-connections-count">{activeGroups.length}</span>
        </div>
        <button type="button" className="home-connections-all" onClick={onOpenAll}>
          查看全部
        </button>
      </div>
      <div className="home-connections-grid connection-card-grid">
        {hasActive ? (
          activeGroups.map((group) => <ConnectionCard key={group.id} group={group} />)
        ) : (
          <div className="empty-block home-connections-empty">暂无活跃链接</div>
        )}
      </div>
    </section>
  )
}
