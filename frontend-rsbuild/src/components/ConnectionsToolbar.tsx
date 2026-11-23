import { For, Show } from 'solid-js'
import { PATH_FILTERS, type PathCounts, type PathFilterOption } from './connectionFilters'

export interface PillGroupProps {
  label: string
  value: string
  options: PathFilterOption[]
  counts?: PathCounts
  onChange: (value: string) => void
}

function PillGroup(props: PillGroupProps) {
  return (
    <div class="connections-pills" role="group" aria-label={props.label}>
      <For each={props.options}>
        {(option) => {
          const count = () => props.counts?.[option.value as keyof PathCounts]
          return (
            <button
              type="button"
              class={['connections-pill', { active: props.value === option.value }]}
              aria-pressed={props.value === option.value ? 'true' : 'false'}
              onClick={() => props.onChange(option.value)}
            >
              <span>{option.label}</span>
              <Show when={typeof count() === 'number'}>
                <em class="num">{count()}</em>
              </Show>
            </button>
          )
        }}
      </For>
    </div>
  )
}

export interface ConnectionsToolbarProps {
  path: string
  onPathChange: (value: string) => void
  counts: PathCounts
}

export function ConnectionsToolbar(props: ConnectionsToolbarProps) {
  return (
    <div class="connections-toolbar">
      <div class="connections-toolbar-row">
        <div class="connections-toolbar-filters">
          <PillGroup
            label="按出口筛选"
            value={props.path}
            options={PATH_FILTERS}
            counts={props.counts}
            onChange={props.onPathChange}
          />
        </div>
      </div>
    </div>
  )
}
