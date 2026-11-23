import { Show, createMemo, createSignal } from 'solid-js'
import { ICON } from '../tokens'
import { ConnectivityCheck } from '../components/ConnectivityCheck'
import { VirtualConnections } from '../components/VirtualConnections'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import {
  connectionOutbound,
  filterConnectionsByPath,
  pathCountsForConnections,
  sortConnections,
  splitConnectionStats,
} from '../components/connectionFilters'
import { connectionDomain } from '../components/siteIcons'
import { buildTerminalRows } from '../domain/terminals'
import { buildAccessPortRows, connectionAccessPortId } from '../domain/accessPorts'
import { formatSpeed } from '../utils'
import type { useAppController } from '../hooks/useAppController'
import { SearchIcon } from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export function ConnectionsView(props: { app: AppController }) {
  const app = () => props.app

  const [pathFilter, setPathFilter] = createSignal<'all' | 'proxy' | 'direct'>('all')
  const [seatFilter, setSeatFilter] = createSignal('')
  const [portFilter, setPortFilter] = createSignal('')
  const [keyword, setKeyword] = createSignal('')

  const connections = createMemo(() => app().connectionsInfo().connections)
  const counts = createMemo(() => pathCountsForConnections(connections()))
  const splitStats = createMemo(() => splitConnectionStats(connections()))

  const terminalRows = createMemo(() => buildTerminalRows({
    terminals: app().terminals(),
    rules: app().rules(),
    connections: connections(),
  }))

  const accessPortRows = createMemo(() => buildAccessPortRows({
    accessPorts: app().accessPorts(),
    nodes: app().nodes(),
    connections: connections(),
  }))

  const seatOptions = createMemo<SelectOption[]>(() => [
    { value: '', label: '全部来源设备' },
    ...terminalRows().map((row) => ({
      value: row.ip,
      label: row.displayName !== row.ip ? `${row.displayName} (${row.ip})` : row.ip,
      badge: row.connectionCount > 0 ? `${row.connectionCount}` : undefined,
      tone: row.failClosed
        ? ('crimson' as const)
        : row.online
          ? ('emerald' as const)
          : ('default' as const),
    })),
  ])

  const portOptions = createMemo<SelectOption[]>(() => [
    { value: '', label: '全部独立端口' },
    ...accessPortRows().map((port) => ({
      value: port.id,
      label: `${port.name} (:${port.port})`,
      badge: port.connectionCount > 0 ? `${port.connectionCount}` : undefined,
      tone: port.blocked ? ('crimson' as const) : ('emerald' as const),
    })),
  ])

  const seatLabels = createMemo<Record<string, string>>(() => {
    const map: Record<string, string> = {}
    for (const row of terminalRows()) {
      map[row.ip] = row.hasSavedProfile || row.dhcpHostname
        ? `${row.displayName} (${row.ip})`
        : row.ip
    }
    return map
  })

  const portLabels = createMemo<Record<string, string>>(() => {
    const map: Record<string, string> = {}
    for (const port of accessPortRows()) {
      map[port.id] = `${port.name} (:${port.port})`
    }
    return map
  })

  const filteredConnections = createMemo(() => {
    let list = filterConnectionsByPath(connections(), pathFilter())
    const seat = seatFilter()
    if (seat) {
      list = list.filter((c) => c.metadata?.sourceIP === seat)
    }
    const portId = portFilter()
    if (portId) {
      list = list.filter((c) => connectionAccessPortId(c) === portId)
    }
    const q = keyword().trim().toLowerCase()
    if (q) {
      list = list.filter((c) => {
        const domain = connectionDomain(c).toLowerCase()
        const rawOut = connectionOutbound(c)
        const out = rawOut.toLowerCase()
        const cleanOut = cleanNodeDisplay(rawOut).name.toLowerCase()
        const src = (c.metadata?.sourceIP || '').toLowerCase()
        const rule = `${c.rule || ''} ${c.rulePayload || ''}`.toLowerCase()
        return (
          domain.includes(q)
          || out.includes(q)
          || cleanOut.includes(q)
          || src.includes(q)
          || rule.includes(q)
        )
      })
    }
    return sortConnections(list)
  })

  const filterKey = createMemo(() => `${pathFilter()}|${seatFilter()}|${portFilter()}|${keyword()}`)

  return (
    <>
      <header class="page-header">
        <div>
          <h1 class="page-title">实时连接</h1>
          <p class="page-subtitle">监控活动连接目标、命中规则与实时上下行速率</p>
        </div>
        <div class="page-actions">
          <span class="status-inline num" style={{ 'margin-right': '4px' }}>
            <span class="status-dot cobalt" />
            <span>代理 ↓ {formatSpeed(splitStats().proxy.downloadSpeed)}</span>
            <span style={{ color: 'var(--ink-muted)' }}>·</span>
            <span class="status-dot emerald" />
            <span>直连 ↓ {formatSpeed(splitStats().direct.downloadSpeed)}</span>
          </span>
          <ConnectivityCheck
            status={app().status()}
            backendUnreachable={app().backendUnreachable()}
          />
        </div>
      </header>

      <div class="studio-toolbar">
        <div class="studio-toolbar-left">
          <div class="seg-rail" role="group" aria-label="出口通道筛选">
            <button
              type="button"
              class={['seg-btn', { active: pathFilter() === 'all' }]}
              aria-pressed={pathFilter() === 'all' ? 'true' : 'false'}
              onClick={() => setPathFilter('all')}
            >
              全部 <span class="num">({counts().all})</span>
            </button>
            <button
              type="button"
              class={['seg-btn', { active: pathFilter() === 'proxy' }]}
              aria-pressed={pathFilter() === 'proxy' ? 'true' : 'false'}
              onClick={() => setPathFilter('proxy')}
            >
              代理 <span class="num">({counts().proxy})</span>
            </button>
            <button
              type="button"
              class={['seg-btn', { active: pathFilter() === 'direct' }]}
              aria-pressed={pathFilter() === 'direct' ? 'true' : 'false'}
              onClick={() => setPathFilter('direct')}
            >
              直连 <span class="num">({counts().direct})</span>
            </button>
          </div>

          <Show when={terminalRows().length > 0}>
            <SelectPopover
              ariaLabel="按来源设备筛选"
              size="sm"
              value={seatFilter()}
              options={seatOptions()}
              searchPlaceholder="搜索设备名称或 IP…"
              onChange={setSeatFilter}
            />
          </Show>

          <Show when={accessPortRows().length > 0}>
            <SelectPopover
              ariaLabel="按独立端口筛选"
              size="sm"
              value={portFilter()}
              options={portOptions()}
              searchPlaceholder="搜索端口名…"
              onChange={setPortFilter}
            />
          </Show>
        </div>

        <div class="studio-search">
          <SearchIcon size={ICON.sm} />
          <input
            type="search"
            aria-label="搜索实时连接"
            placeholder="搜索域名、IP、进程或节点…"
            value={keyword()}
            onInput={(e) => setKeyword(e.currentTarget.value)}
          />
        </div>
      </div>

      <section class="surface-box" aria-label="实时连接列表">
        <Show
          when={app().status().ready}
          fallback={
            <div class="empty-state">
              代理服务当前未运行，启动服务后可在此查看实时连接。
            </div>
          }
        >
          <Show
            when={filteredConnections().length > 0}
            fallback={
              <div class="empty-state">
                当前筛选条件下暂无活动连接。
              </div>
            }
          >
            <VirtualConnections
              connections={filteredConnections()}
              filterKey={filterKey()}
              seatLabels={seatLabels()}
              portLabels={portLabels()}
            />
          </Show>
        </Show>
      </section>
    </>
  )
}
