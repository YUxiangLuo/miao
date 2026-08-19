import { ArrowDown, ArrowUp } from 'lucide-react'
import { ICON } from '../tokens'
import { formatBytes, formatSpeed } from '../utils'
import type { EnrichedConnection } from '../types/clash'
import { splitConnectionStats, type PathStats } from './connectionFilters'
import { AnimatedValue } from './connectionUi'

function laneShare(lane: PathStats, other: PathStats): number {
  const mine = lane.downloadSpeed + lane.uploadSpeed
  const total = mine + other.downloadSpeed + other.uploadSpeed
  return total > 0 ? mine / total : 0
}

function PathLane({ tone, label, stats, share }: {
  tone: 'info' | 'success'
  label: string
  stats: PathStats
  /** 本通道占两通道合计速率的比例（0-1） */
  share: number
}) {
  return (
    <div className="path-lane">
      <span className={`badge ${tone} path-chip`}>{label}</span>
      <span className="path-speed">
        <small className="tone-download">
          <ArrowDown size={ICON.sm} />
          <AnimatedValue value={formatSpeed(stats.downloadSpeed)} />
        </small>
        <small className="tone-upload">
          <ArrowUp size={ICON.sm} />
          <AnimatedValue value={formatSpeed(stats.uploadSpeed)} />
        </small>
      </span>
      <span className="path-total">
        <span className="path-total-label">累计</span>
        <small className="tone-download">
          <ArrowDown size={ICON.xs} />
          <AnimatedValue value={formatBytes(stats.download)} />
        </small>
        <small className="tone-upload">
          <ArrowUp size={ICON.xs} />
          <AnimatedValue value={formatBytes(stats.upload)} />
        </small>
      </span>
      <span className="path-count">{stats.count} 条链接</span>
      <i className={`path-share ${tone}`} aria-hidden="true">
        <i style={{ width: `${Math.round(share * 100)}%` }} />
      </i>
    </div>
  )
}

/** 直连 / 代理双通道汇总卡：速率、累计流量、链接数与占比条 */
export function ConnectionStats({ connections }: { connections: EnrichedConnection[] }) {
  const stats = splitConnectionStats(connections)
  return (
    <div className="path-stats">
      <PathLane
        tone="info"
        label="代理"
        stats={stats.proxy}
        share={laneShare(stats.proxy, stats.direct)}
      />
      <PathLane
        tone="success"
        label="直连"
        stats={stats.direct}
        share={laneShare(stats.direct, stats.proxy)}
      />
    </div>
  )
}
