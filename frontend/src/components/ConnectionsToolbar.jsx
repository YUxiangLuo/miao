import { Search, X } from 'lucide-react'
import { ICON } from '../tokens.js'
import { classNames } from '../utils.js'
import { PATH_FILTERS } from './connectionFilters.js'

function PillGroup({ label, value, options, counts, onChange }) {
  return (
    <div className="connections-pills" role="group" aria-label={label}>
      {options.map((option) => {
        const count = counts?.[option.value]
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

export function ConnectionsToolbar({
  query,
  onQueryChange,
  path,
  onPathChange,
  counts,
  resultCount,
  totalCount,
}) {
  const hasQuery = query.trim().length > 0
  const filtered = hasQuery || path !== 'all'

  return (
    <div className="connections-toolbar">
      <div className="connections-toolbar-row">
        <div className="connections-toolbar-filters">
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
            onChange={(event) => onQueryChange(event.target.value)}
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
