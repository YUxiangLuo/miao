import type { ChangeEvent } from 'react'
import { Search, X } from 'lucide-react'
import { ICON } from '../tokens'
import { classNames } from '../utils'
import type { ConnectionDimension } from '../types/clash'
import { DIMENSION_FILTERS, PATH_FILTERS, type PathCounts, type PathFilterOption } from './connectionFilters'

export interface PillGroupProps {
  label: string
  value: string
  options: PathFilterOption[]
  counts?: PathCounts
  onChange: (value: string) => void
}

function PillGroup({ label, value, options, counts, onChange }: PillGroupProps) {
  return (
    <div className="connections-pills" role="group" aria-label={label}>
      {options.map((option) => {
        const count = counts?.[option.value as keyof PathCounts]
        return (
          <button
            key={option.value}
            type="button"
            className={classNames('connections-pill', value === option.value && 'active')}
            aria-pressed={value === option.value}
            onClick={() => onChange(option.value)}
          >
            <span>{option.label}</span>
            {typeof count === 'number' && <em>{count}</em>}
          </button>
        )
      })}
    </div>
  )
}

export interface ConnectionsToolbarProps {
  dimension: ConnectionDimension
  onDimensionChange: (value: ConnectionDimension) => void
  query: string
  onQueryChange: (value: string) => void
  path: string
  onPathChange: (value: string) => void
  counts: PathCounts
  resultCount: number
  totalCount: number
}

export function ConnectionsToolbar({
  dimension,
  onDimensionChange,
  query,
  onQueryChange,
  path,
  onPathChange,
  counts,
  resultCount,
  totalCount,
}: ConnectionsToolbarProps) {
  const hasQuery = query.trim().length > 0
  const filtered = hasQuery || path !== 'all'

  return (
    <div className="connections-toolbar">
      <div className="connections-toolbar-row">
        <div className="connections-toolbar-filters">
          <PillGroup
            label="聚合维度"
            value={dimension}
            options={DIMENSION_FILTERS}
            onChange={(value) => onDimensionChange(value as ConnectionDimension)}
          />
          <PillGroup
            label="按出口筛选"
            value={path}
            options={PATH_FILTERS}
            counts={counts}
            onChange={onPathChange}
          />
        </div>

        <label className="connections-search">
          <Search size={ICON.sm} />
          <input
            data-autofocus
            type="search"
            value={query}
            onChange={(event: ChangeEvent<HTMLInputElement>) => onQueryChange(event.target.value)}
            placeholder="搜索域名、规则或出口"
            aria-label="搜索链接"
          />
          {hasQuery && (
            <button
              type="button"
              className="connections-search-clear"
              onClick={() => onQueryChange('')}
              aria-label="清除搜索"
            >
              <X size={ICON.xs} />
            </button>
          )}
          {filtered && (
            <span className="connections-search-count">
              {resultCount} / {totalCount}
            </span>
          )}
        </label>
      </div>
    </div>
  )
}
