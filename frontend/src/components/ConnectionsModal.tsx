import { useId, useMemo, useState } from 'react'
import {
  Activity,
  AppWindow,
  ArrowDown,
  ArrowUp,
  Gauge,
  Globe,
  HardDriveDownload,
  Split,
  X,
} from 'lucide-react'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { formatBytes, formatSpeed } from '../utils'
import { ConnectionsToolbar } from './ConnectionsToolbar'
import { AnimatedValue } from './ConnectionCard'
import { ConnectionRow } from './ConnectionRow'
import {
  buildGroupRows,
  filterGroupRows,
  pathCountsForRows,
  sortGroupRows,
} from './connectionFilters'

import type { StatusData } from '../types/api'
import type { ConnectionDimension, ConnectionsInfo } from '../types/clash'

export interface ConnectionsModalProps {
  open: boolean
  status: StatusData
  data: ConnectionsInfo
  loading: boolean
  error: string
  onClose: () => void
}

const DIMENSION_LABELS: Record<ConnectionDimension, string> = {
  site: '站点',
  process: '进程',
  outbound: '出口',
}

/** 统计卡首格图标随维度切换 */
const DIMENSION_ICONS = {
  site: Globe,
  process: AppWindow,
  outbound: Split,
} as const

export function ConnectionsModal({
  open,
  status,
  data,
  loading,
  error,
  onClose,
}: ConnectionsModalProps) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [dimension, setDimension] = useState<ConnectionDimension>('site')
  const [query, setQuery] = useState('')
  const [path, setPath] = useState('all')
  const [expandedId, setExpandedId] = useState<string | null>(null)

  const connections = useMemo(() => {
    return Array.isArray(data?.connections) ? data.connections : []
  }, [data?.connections])
  const uploadTotal = Number(data?.uploadTotal || connections.reduce((sum, item) => sum + Number(item.upload || 0), 0))
  const downloadTotal = Number(data?.downloadTotal || connections.reduce((sum, item) => sum + Number(item.download || 0), 0))
  const uploadSpeed = connections.reduce((sum, item) => sum + Number(item.uploadSpeed || 0), 0)
  const downloadSpeed = connections.reduce((sum, item) => sum + Number(item.downloadSpeed || 0), 0)

  // 三维聚合：站点 / 进程 / 出口，全部归一为 GroupRow 后走同一套筛选排序
  const rows = useMemo(() => buildGroupRows(dimension, connections), [dimension, connections])
  const searchedRows = useMemo(
    () => filterGroupRows(rows, { query, path: 'all' }),
    [rows, query],
  )
  const visibleRows = useMemo(
    () => sortGroupRows(filterGroupRows(searchedRows, { path })),
    [path, searchedRows],
  )
  const pathCounts = useMemo(() => pathCountsForRows(searchedRows), [searchedRows])
  const maxSpeed = useMemo(
    () => visibleRows.reduce((max, row) => Math.max(max, row.downloadSpeed + row.uploadSpeed), 0),
    [visibleRows],
  )
  // 维度切换后展开态按旧维度 id 已无意义，收起避免错位
  const handleDimensionChange = (next: ConnectionDimension) => {
    setDimension(next)
    setExpandedId(null)
  }
  const DimensionIcon = DIMENSION_ICONS[dimension]

  if (!open) return null

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card connections-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="connections-header">
          <div className="connections-header-title">
            <Activity size={ICON.lg} className="icon-accent" />
            <h3 id={titleId}>链接统计</h3>
            {status.running && (
              <span className="badge connections-live-badge">
                <i />
                {rows.length} 个{DIMENSION_LABELS[dimension]} · {connections.length} 条链接
              </span>
            )}
          </div>
          <div className="connections-header-actions">
            <button className="icon-button" onClick={onClose} title="关闭 (Esc)" aria-label="关闭链接统计">
              <X size={ICON.md} />
            </button>
          </div>
        </header>

        {!status.running ? (
          <div className="connections-empty">服务未运行，暂无链接统计。</div>
        ) : (
          <div className="connections-body">
            <div className="connection-stat-grid">
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <DimensionIcon size={ICON.xs} />
                  {DIMENSION_LABELS[dimension]}
                </span>
                <strong className="connection-stat-value">
                  <AnimatedValue value={rows.length} />
                </strong>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <Gauge size={ICON.xs} />
                  实时速度
                </span>
                <strong className="connection-stat-value connection-stat-pair">
                  <span className="tone-download">
                    <ArrowDown size={ICON.xs} />
                    <AnimatedValue value={formatSpeed(downloadSpeed)} />
                  </span>
                  <span className="tone-upload">
                    <ArrowUp size={ICON.xs} />
                    <AnimatedValue value={formatSpeed(uploadSpeed)} />
                  </span>
                </strong>
              </div>
              <div className="connection-stat">
                <span className="connection-stat-label">
                  <HardDriveDownload size={ICON.xs} />
                  累计流量
                </span>
                <strong className="connection-stat-value connection-stat-pair">
                  <span className="tone-download">
                    <ArrowDown size={ICON.xs} />
                    <AnimatedValue value={formatBytes(downloadTotal)} />
                  </span>
                  <span className="tone-upload">
                    <ArrowUp size={ICON.xs} />
                    <AnimatedValue value={formatBytes(uploadTotal)} />
                  </span>
                </strong>
              </div>
            </div>

            {error && <div className="connections-error">{error}</div>}

            <div className="connections-main">
              <ConnectionsToolbar
                dimension={dimension}
                onDimensionChange={handleDimensionChange}
                query={query}
                onQueryChange={setQuery}
                path={path}
                onPathChange={setPath}
                counts={pathCounts}
                resultCount={visibleRows.length}
                totalCount={rows.length}
              />

              {visibleRows.length > 0 ? (
                <div className="conn-rows">
                  {visibleRows.map((row) => (
                    <ConnectionRow
                      key={row.id}
                      row={row}
                      maxSpeed={maxSpeed}
                      expanded={expandedId === row.id}
                      onToggle={() => setExpandedId(expandedId === row.id ? null : row.id)}
                    />
                  ))}
                </div>
              ) : (
                <div className="connections-empty inline">
                  {loading && connections.length === 0 ? '加载中…' : `暂无匹配${DIMENSION_LABELS[dimension]}`}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
