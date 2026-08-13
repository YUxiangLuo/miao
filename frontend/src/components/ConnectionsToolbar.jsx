import { Search, X } from 'lucide-react'
import { classNames } from '../utils.js'
import { PATH_FILTERS, SORT_OPTIONS } from './connectionFilters.js'

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
  sortKey,
  onSortChange,
  counts,
  resultCount,
  totalCount,
}) {
  const hasQuery = query.trim().length > 0
  const filtered = hasQuery || path !== 'all'

  return (
    <div className="connections-toolbar">
      <label className="connections-search">
        <Search size={14} />
        <input
          data-autofocus
          type="search"
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          placeholder="搜索域名、规则或出口"
          aria-label="搜索连接"
        />
        {hasQuery && (
          <button
            type="button"
            className="connections-search-clear"
            onClick={() => onQueryChange('')}
            aria-label="清除搜索"
          >
            <X size={13} />
          </button>
        )}
        <span className="connections-search-count">
          {filtered ? `${resultCount} / ${totalCount}` : `${totalCount} 个站点`}
        </span>
      </label>

      <div className="connections-toolbar-row">
        <PillGroup
          label="按出口筛选"
          value={path}
          options={PATH_FILTERS}
          counts={counts}
          onChange={onPathChange}
        />
        <PillGroup
          label="排序"
          value={sortKey}
          options={SORT_OPTIONS}
          onChange={onSortChange}
        />
      </div>
    </div>
  )
}
