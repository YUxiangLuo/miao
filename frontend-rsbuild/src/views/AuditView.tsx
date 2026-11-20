import { For, Show, createMemo, createSignal } from 'solid-js'
import { ICON } from '../tokens'
import { ConnectivityCheck } from '../components/ConnectivityCheck'
import { VirtualConnections } from '../components/VirtualConnections'
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
import { classNames, formatBytes, formatSpeed } from '../utils'
import type { useAppController } from '../hooks/useAppController'
import { ActivityIcon, SearchIcon } from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export function AuditView(props: { app: AppController }) {
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
        const out = connectionOutbound(c).toLowerCase()
        const src = (c.metadata?.sourceIP || '').toLowerCase()
        const rule = `${c.rule || ''} ${c.rulePayload || ''}`.toLowerCase()
        return domain.includes(q) || out.includes(q) || src.includes(q) || rule.includes(q)
      })
    }
    return sortConnections(list)
  })

  const filterKey = createMemo(() => `${pathFilter()}|${seatFilter()}|${portFilter()}|${keyword()}`)

  return (
    <section class="hg-card">
      <div class="hg-card-header">
        <div class="hg-card-title-group">
          <h2>
            <ActivityIcon size={ICON.md} class="icon-accent" />
            实时连接与跨境流量审计 ({filteredConnections().length} / {counts().all})
          </h2>
          <p class="num">
            海外通道 ↓ {formatSpeed(splitStats().proxy.downloadSpeed)} (累计 {formatBytes(splitStats().proxy.download)}) · 国内直连 ↓ {formatSpeed(splitStats().direct.downloadSpeed)} (累计 {formatBytes(splitStats().direct.download)})
          </p>
        </div>
        <div class="hg-card-actions">
          <ConnectivityCheck
            status={app().status()}
            backendUnreachable={app().backendUnreachable()}
          />
        </div>
      </div>

      <div class="hg-card-body" style={{ 'padding-bottom': '10px' }}>
        <div class="hg-toolbar">
          <div class="hg-toolbar-group">
            <div class="hg-segmented" role="group" aria-label="出口通道筛选">
              {([
                ['all', `全部 (${counts().all})`],
                ['proxy', `海外通道 (${counts().proxy})`],
                ['direct', `国内直连 (${counts().direct})`],
              ] as const).map(([key, label]) => (
                <button
                  type="button"
                  class={classNames('hg-segment', pathFilter() === key && 'active')}
                  aria-pressed={pathFilter() === key ? 'true' : 'false'}
                  onClick={() => setPathFilter(key)}
                >
                  {label}
                </button>
              ))}
            </div>

            <select
              class="hg-select"
              aria-label="按办公工位筛选连接"
              value={seatFilter()}
              onChange={(e) => setSeatFilter(e.currentTarget.value)}
            >
              <option value="">全部办公工位</option>
              <For each={terminalRows()}>
                {(row) => (
                  <option value={row.ip}>
                    {row.displayName} ({row.ip})
                  </option>
                )}
              </For>
            </select>

            <Show when={accessPortRows().length > 0}>
              <select
                class="hg-select"
                aria-label="按多店铺专属端口筛选连接"
                value={portFilter()}
                onChange={(e) => setPortFilter(e.currentTarget.value)}
              >
                <option value="">全部店铺端口</option>
                <For each={accessPortRows()}>
                  {(port) => (
                    <option value={port.id}>
                      {port.name} (:{port.port})
                    </option>
                  )}
                </For>
              </select>
            </Show>
          </div>

          <div class="hg-search">
            <SearchIcon size={ICON.sm} />
            <input
              type="search"
              aria-label="搜索审计连接"
              placeholder="搜索目标域名、IP、出口专线或规则…"
              value={keyword()}
              onInput={(e) => setKeyword(e.currentTarget.value)}
            />
          </div>
        </div>
      </div>

      <div class="hg-card-body" style={{ 'padding-top': '4px' }}>
        <Show
          when={app().status().ready}
          fallback={
            <div class="empty-block">
              网关转发服务当前未运行，启动服务后即可实时审计全公司各工位与店铺端口的访问连接。
            </div>
          }
        >
          <Show
            when={filteredConnections().length > 0}
            fallback={
              <div class="empty-block">
                没有匹配当前筛选条件的实时连接。
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
      </div>
    </section>
  )
}
