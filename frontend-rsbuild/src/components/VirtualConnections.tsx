import { useLayoutEffect, useMemo, useRef, useState } from 'react'
import { CONNECTION_ROW_HEIGHT, CONNECTION_ROW_GAP } from '../tokens'
import { useFlipContainer } from '../hooks/useFlip'
import type { EnrichedConnection } from '../types/clash'
import { ConnectionRow } from './ConnectionRow'

const OVERSCAN = 5
const INITIAL_VISIBLE_ROWS = 12
const STEP = CONNECTION_ROW_HEIGHT + CONNECTION_ROW_GAP

export function VirtualConnections({ connections }: { connections: EnrichedConnection[] }) {
  const viewport = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(STEP * INITIAL_VISIBLE_ROWS)
  const [scrollTop, setScrollTop] = useState(0)
  const visibleCount = Math.ceil(height / STEP)
  const first = Math.min(Math.floor(scrollTop / STEP), Math.max(0, connections.length - visibleCount))
  const start = Math.max(0, first - OVERSCAN)
  const end = Math.min(connections.length, first + visibleCount + OVERSCAN)
  const visible = useMemo(() => connections.slice(start, end), [connections, start, end])
  const order = `${start}:${JSON.stringify(visible.map(connection => connection.id))}`
  const rowsRef = useFlipContainer<HTMLDivElement>(true, order)

  useLayoutEffect(() => {
    const element = viewport.current
    if (!element) return
    const update = () => { if (element.clientHeight > 0) setHeight(element.clientHeight) }
    update()
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', update)
      return () => window.removeEventListener('resize', update)
    }
    const observer = new ResizeObserver(update)
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  useLayoutEffect(() => {
    const element = viewport.current
    if (!element) return
    const maximum = Math.max(0, connections.length * STEP - CONNECTION_ROW_GAP - height)
    if (element.scrollTop > maximum) {
      element.scrollTop = maximum
      setScrollTop(maximum)
    }
  }, [connections.length, height])

  return (
    <div className="conn-rows" ref={viewport} tabIndex={0} role="list" aria-label="连接列表"
      onScroll={event => setScrollTop(event.currentTarget.scrollTop)}>
      <div className="conn-virtual-content" ref={rowsRef}
        style={{ height: Math.max(0, connections.length * STEP - CONNECTION_ROW_GAP) }}>
        {visible.map((connection, index) => (
          <div className="conn-virtual-item" key={connection.id} style={{ top: (start + index) * STEP }}
            data-flip-key={connection.id}>
            <ConnectionRow connection={connection} position={start + index + 1} total={connections.length} />
          </div>
        ))}
      </div>
    </div>
  )
}
