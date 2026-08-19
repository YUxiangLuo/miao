import { useId, useMemo, useState } from 'react'
import { Activity, X } from 'lucide-react'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { ConnectionsToolbar } from './ConnectionsToolbar'
import { ConnectionRow } from './ConnectionRow'
import { ConnectionStats } from './ConnectionStats'
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
            <ConnectionStats connections={connections} />

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
