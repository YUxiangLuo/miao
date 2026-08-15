import { useMemo, useState } from 'react'
import { Activity } from 'lucide-react'
import { ConnectionCard } from './ConnectionCard.jsx'
import { groupConnections, groupSpeed, sortConnectionGroups } from './connectionFilters.js'

export function HomeConnections({ status, data, onOpenAll }) {
  const [expandedId, setExpandedId] = useState(null)
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

  if (!status.running || activeGroups.length === 0) return null

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
        {activeGroups.map((group) => (
          <ConnectionCard
            key={group.id}
            group={group}
            expanded={expandedId === group.id}
            onToggle={() => setExpandedId(expandedId === group.id ? null : group.id)}
          />
        ))}
      </div>
    </section>
  )
}
