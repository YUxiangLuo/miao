import { classNames } from '../utils'
import { PATH_FILTERS, type PathCounts, type PathFilterOption } from './connectionFilters'

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
  path: string
  onPathChange: (value: string) => void
  counts: PathCounts
}

/** 链接统计工具栏：只保留直连/代理路径筛选 */
export function ConnectionsToolbar({ path, onPathChange, counts }: ConnectionsToolbarProps) {
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
      </div>
    </div>
  )
}
