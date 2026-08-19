import { useId, useMemo, useState } from 'react'
import {
  Activity,
  ArrowDown,
  ArrowUp,
  Gauge,
  HardDriveDownload,
  Link2,
  X,
} from 'lucide-react'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { formatBytes, formatSpeed } from '../utils'
import { ConnectionsToolbar } from './ConnectionsToolbar'
import { AnimatedValue } from './ConnectionCard'
import { ConnectionRow } from './ConnectionRow'
import {
  filterConnectionsByPath,
  pathCountsForConnections,
  sortConnections,
} from './connectionFilters'

import type { StatusData } from '../types/api'
import type { ConnectionsInfo } from '../types/clash'

export interface ConnectionsModalProps {
  open: boolean
  status: StatusData
  data: ConnectionsInfo
  loading: boolean
  error: string
  onClose: () => void
}

/** 链接统计：以链接为单位的全量列表，仅保留直连/代理筛选 */
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
  const [path, setPath] = useState('all')

  const connections = useMemo(() => {
    return Array.isArray(data?.connections) ? data.connections : []
  }, [data?.connections])
  const uploadTotal = Number(data?.uploadTotal || connections.reduce((sum, item) => sum + Number(item.upload || 0), 0))
  const downloadTotal = Number(data?.downloadTotal || connections.reduce((sum, item) => sum + Number(item.download || 0), 0))
  const uploadSpeed = connections.reduce((sum, item) => sum + Number(item.uploadSpeed || 0), 0)
  const downloadSpeed = connections.reduce((sum, item) => sum + Number(item.downloadSpeed || 0), 0)

  const visibleConnections = useMemo(
    () => sortConnections(filterConnectionsByPath(connections, path)),
    [connections, path],
  )
  const pathCounts = useMemo(() => pathCountsForConnections(connections), [connections])

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
                {connections.length} 条链接
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
                  <Link2 size={ICON.xs} />
                  链接
                </span>
                <strong className="connection-stat-value">
                  <AnimatedValue value={connections.length} />
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
                path={path}
                onPathChange={setPath}
                counts={pathCounts}
              />

              {visibleConnections.length > 0 ? (
                <div className="conn-rows">
                  {visibleConnections.map((connection) => (
                    <ConnectionRow
                      key={connection.id}
                      connection={connection}
                    />
                  ))}
                </div>
              ) : (
                <div className="connections-empty inline">
                  {loading && connections.length === 0 ? '加载中…' : '暂无匹配链接'}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
