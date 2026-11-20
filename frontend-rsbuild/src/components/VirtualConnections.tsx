import { For, createEffect, createMemo, createSignal } from 'solid-js'
import { CONNECTION_ROW_HEIGHT, CONNECTION_ROW_GAP } from '../tokens'
import { useFlipContainer } from '../hooks/useFlip'
import type { EnrichedConnection } from '../types/clash'
import { ConnectionRow } from './ConnectionRow'

const OVERSCAN = 5
const INITIAL_VISIBLE_ROWS = 12
const STEP = CONNECTION_ROW_HEIGHT + CONNECTION_ROW_GAP

export function VirtualConnections(props: {
  connections: EnrichedConnection[]
  filterKey?: string
  seatLabels?: Record<string, string>
  portLabels?: Record<string, string>
}) {
  const [viewport, setViewport] = createSignal<HTMLDivElement | undefined>()
  const [height, setHeight] = createSignal(STEP * INITIAL_VISIBLE_ROWS)
  const [scrollTop, setScrollTop] = createSignal(0)

  const visibleCount = createMemo(() => Math.ceil(height() / STEP))
  const first = createMemo(() => Math.min(Math.floor(scrollTop() / STEP), Math.max(0, props.connections.length - visibleCount())))
  const start = createMemo(() => Math.max(0, first() - OVERSCAN))
  const end = createMemo(() => Math.min(props.connections.length, first() + visibleCount() + OVERSCAN))
  const visible = createMemo(() => props.connections.slice(start(), end()))
  const order = createMemo(() => `${props.filterKey ?? ''}:${start()}:${JSON.stringify(visible().map((c) => c.id))}`)
  const rowsRef = useFlipContainer<HTMLDivElement>(() => true, order)

  createEffect(
    () => viewport(),
    (element) => {
      if (!element) return
      const update = () => {
        if (element.clientHeight > 0) setHeight(element.clientHeight)
      }
      update()
      if (typeof ResizeObserver === 'undefined') {
        window.addEventListener('resize', update)
        return () => window.removeEventListener('resize', update)
      }
      const observer = new ResizeObserver(update)
      observer.observe(element)
      return () => observer.disconnect()
    },
  )

  createEffect(
    () => ({ element: viewport(), len: props.connections.length, h: height(), key: props.filterKey }),
    ({ element, len, h }) => {
      if (!element) return
      const maximum = Math.max(0, len * STEP - CONNECTION_ROW_GAP - h)
      if (element.scrollTop > maximum) {
        element.scrollTop = maximum
        setScrollTop(maximum)
      }
    },
  )

  return (
    <div
      class="conn-rows"
      ref={(el) => setViewport(() => el)}
      tabindex={0}
      role="list"
      aria-label="连接列表"
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
    >
      <div
        class="conn-virtual-content"
        ref={rowsRef}
        style={{ height: `${Math.max(0, props.connections.length * STEP - CONNECTION_ROW_GAP)}px` }}
      >
        <For each={visible()} keyed={(connection) => connection.id}>
          {(connection, index) => (
            <div
              class="conn-virtual-item"
              style={{ top: `${(start() + index()) * STEP}px` }}
              data-flip-key={connection().id}
            >
              <ConnectionRow
                connection={connection()}
                position={start() + index() + 1}
                total={props.connections.length}
                seatLabels={props.seatLabels}
                portLabels={props.portLabels}
              />
            </div>
          )}
        </For>
      </div>
    </div>
  )
}
