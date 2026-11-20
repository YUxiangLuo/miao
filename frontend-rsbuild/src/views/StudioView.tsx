import { For, Show, createMemo } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Button } from '../components/ui'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import { splitConnectionStats } from '../components/connectionFilters'
import { classNames, formatDelay, formatSpeed } from '../utils'
import { buildTerminalRows } from '../domain/terminals'
import { buildAccessPortRows } from '../domain/accessPorts'
import { summarizeActiveTradePlatforms } from '../domain/tradeCatalog'
import type { NodeSelect } from '../types/api'
import type { useAppController } from '../hooks/useAppController'

type AppController = ReturnType<typeof useAppController>

type SignalDotTone = 'emerald' | 'cobalt' | 'amber' | 'crimson' | 'muted'

interface StudioLedgerRow {
  key: string
  title: string
  badge?: string
  address: string
  routeCode: string | null
  routeLabel: string
  dotTone: SignalDotTone
  danger: boolean
  isDedicated: boolean
  downloadSpeed: number
  uploadSpeed: number
  connectionCount: number
}

const AUTO_SELECT_LABELS: Record<Exclude<NodeSelect, 'manual'>, string> = {
  fastest_hk: '自动优选 · HK',
  fastest_jp: '自动优选 · JP',
  fastest_sg: '自动优选 · SG',
  fastest_tw: '自动优选 · TW',
  fastest_us: '自动优选 · US',
}

function formatOutboundCompact(raw: string): string {
  const normalized = raw.trim().toLowerCase()
  if (!normalized || normalized === 'direct') return '国内直连'
  if (normalized === 'proxy') return '公共出海池'
  if (normalized === 'reject' || normalized === 'block') return '已阻断'
  const cleaned = cleanNodeDisplay(raw)
  return cleaned.code ? `${cleaned.code} · ${cleaned.name}` : cleaned.name
}

export function StudioView(props: { app: AppController }): JSX.Element {
  const app = () => props.app

  const splitStats = createMemo(() => splitConnectionStats(app().connectionsInfo().connections))

  const terminalRows = createMemo(() => buildTerminalRows({
    terminals: app().terminals(),
    rules: app().rules(),
    connections: app().connectionsInfo().connections,
  }))

  const accessPortRows = createMemo(() => buildAccessPortRows({
    accessPorts: app().accessPorts(),
    nodes: app().nodes(),
    connections: app().connectionsInfo().connections,
  }))

  const activePlatforms = createMemo(() => summarizeActiveTradePlatforms(app().connectionsInfo().connections))

  const failClosedSeats = createMemo(() => terminalRows().filter((row) => row.failClosed))
  const failClosedPorts = createMemo(() => accessPortRows().filter((row) => row.blocked))
  const failClosedCount = createMemo(() => failClosedSeats().length + failClosedPorts().length)
  const dedicatedSeatCount = createMemo(() => terminalRows().filter((row) => row.mode === 'dedicated').length)

  const downSpeed = createMemo(
    () => app().traffic().down ?? (splitStats().proxy.downloadSpeed + splitStats().direct.downloadSpeed),
  )
  const upSpeed = createMemo(
    () => app().traffic().up ?? (splitStats().proxy.uploadSpeed + splitStats().direct.uploadSpeed),
  )

  const totalConnections = createMemo(() => splitStats().proxy.count + splitStats().direct.count)
  const overseasRatioPct = createMemo(() => {
    const total = totalConnections()
    if (total <= 0) return 50
    return Math.min(100, Math.max(0, Math.round((splitStats().proxy.count / total) * 100)))
  })

  const proxyNodes = createMemo(() => app().primaryGroup()?.all ?? [])
  const currentNodeName = createMemo(() => app().primaryGroup()?.now ?? '')
  const currentNodeDisplay = createMemo(() => cleanNodeDisplay(currentNodeName()))
  const requestedSelect = createMemo<NodeSelect>(
    () => app().status().requested_node_select || app().status().node_select || 'manual',
  )
  const isManualExit = createMemo(
    () => requestedSelect() === 'manual' && app().status().node_select === 'manual',
  )

  const primaryExitOptions = createMemo<SelectOption[]>(() => {
    const delays = app().delays()
    const protocols = app().nodeProtocols()
    const current = currentNodeName()
    return proxyNodes().map((nodeName) => {
      const delay = delays[nodeName]
      const badge = delay !== undefined ? formatDelay(delay) : protocols[nodeName]
      const tone: SelectOption['tone'] = delay !== undefined && delay < 0
        ? 'crimson'
        : current === nodeName
          ? 'emerald'
          : 'cobalt'
      return {
        value: nodeName,
        label: nodeName,
        badge,
        tone,
      }
    })
  })

  const ledgerRows = createMemo<StudioLedgerRow[]>(() => {
    const portItems: StudioLedgerRow[] = accessPortRows().map((port) => {
      const cleaned = cleanNodeDisplay(port.outbound)
      return {
        key: `port:${port.id}`,
        title: port.name,
        badge: '店铺端口',
        address: `:${port.port}`,
        routeCode: cleaned.code,
        routeLabel: port.blocked ? `离线熔断 · ${cleaned.name}` : `独享 · ${cleaned.name}`,
        dotTone: port.blocked ? 'crimson' : 'emerald',
        danger: port.blocked,
        isDedicated: true,
        downloadSpeed: port.downloadSpeed,
        uploadSpeed: port.uploadSpeed,
        connectionCount: port.connectionCount,
      }
    })

    const deviceItems: StudioLedgerRow[] = terminalRows().map((row) => {
      const cleaned = row.target ? cleanNodeDisplay(row.target) : { code: null, name: '' }
      let routeLabel = '公共出海池'
      let routeCode: string | null = null
      let dotTone: SignalDotTone = 'cobalt'

      if (row.failClosed) {
        routeCode = cleaned.code
        routeLabel = cleaned.name ? `离线熔断 · ${cleaned.name}` : '离线熔断'
        dotTone = 'crimson'
      } else if (row.mode === 'dedicated' && cleaned.name) {
        routeCode = cleaned.code
        routeLabel = `独享 · ${cleaned.name}`
        dotTone = 'emerald'
      } else if (row.mode === 'block_overseas') {
        routeLabel = '仅国内办公'
        dotTone = 'amber'
      } else if (row.mode === 'direct') {
        routeLabel = '全量直连'
        dotTone = 'muted'
      }

      return {
        key: `device:${row.ip}`,
        title: row.displayName,
        badge: row.department,
        address: row.ip,
        routeCode,
        routeLabel,
        dotTone,
        danger: row.failClosed,
        isDedicated: row.mode === 'dedicated',
        downloadSpeed: row.downloadSpeed,
        uploadSpeed: row.uploadSpeed,
        connectionCount: row.connectionCount,
      }
    })

    return [...portItems, ...deviceItems]
      .sort((a, b) => (
        Number(b.danger) - Number(a.danger)
        || (b.downloadSpeed + b.uploadSpeed) - (a.downloadSpeed + a.uploadSpeed)
        || Number(b.connectionCount > 0) - Number(a.connectionCount > 0)
        || Number(b.isDedicated) - Number(a.isDedicated)
      ))
      .slice(0, 8)
  })

  return (
    <>
      <Show when={failClosedCount() > 0}>
        <div class="circuit-banner danger" role="alert">
          <div class="circuit-banner-main">
            <span class="status-dot crimson pulse" aria-hidden="true" />
            <span>
              {failClosedCount()} 项独享绑定因目标专线离线已触发防关联熔断（国内直连不受影响）
            </span>
          </div>
          <div class="circuit-banner-actions">
            <Button tone="secondary" size="sm" onClick={() => app().setActiveNav('endpoints')}>
              查看终端与店铺
            </Button>
            <Button tone="ghost" size="sm" onClick={() => app().setActiveNav('routes')}>
              排查全球线路
            </Button>
          </div>
        </div>
      </Show>

      <Show when={app().status().warnings.length > 0}>
        <div class="circuit-banner warning" role="status">
          <div class="circuit-banner-main">
            <span class="status-dot amber" aria-hidden="true" />
            <span>{app().status().warnings.map((w) => w.message).join('；')}</span>
          </div>
        </div>
      </Show>

      <section class="surface-box signal-hero" aria-label="实时信号流向">
        <div class="signal-grid">
          {/* Stage 1: LAN Ingress */}
          <div class="signal-stage">
            <div class="signal-stage-head">
              <span class="status-inline">
                <span
                  class={classNames(
                    'status-dot',
                    terminalRows().length > 0 || accessPortRows().length > 0 ? 'emerald' : 'muted',
                  )}
                  aria-hidden="true"
                />
                <span>局域网入口</span>
              </span>
              <span class="mono-tag">LAN</span>
            </div>

            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '4px' }}>
              <div class="signal-metric num">
                <span>{terminalRows().length} 台设备</span>
              </div>
              <div class="row-sub num" style={{ margin: 0, 'font-size': '13px' }}>
                {accessPortRows().length} 个店铺端口
              </div>
            </div>

            <div class="signal-stage-foot">
              <span class="num">{dedicatedSeatCount()} 台锁定独享 IP</span>
              <button
                type="button"
                class="btn-link"
                onClick={() => app().setActiveNav('endpoints')}
              >
                配线管理 →
              </button>
            </div>
          </div>

          {/* Stage 2: Harbor Split Core */}
          <div class="signal-stage signal-core">
            <div class="signal-stage-head">
              <span class="status-inline">
                <span
                  class={classNames(
                    'status-dot',
                    app().status().ready ? 'emerald pulse' : app().status().initializing ? 'amber' : 'crimson',
                  )}
                  aria-hidden="true"
                />
                <span>海港分流核心</span>
              </span>
              <span class="mono-tag">
                {app().status().route_mode === 'rule' ? '大陆直连优先' : '全量出海'}
              </span>
            </div>

            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '8px' }}>
              <svg
                class={classNames('signal-wire-svg', app().status().ready && 'active')}
                viewBox="0 0 240 36"
                fill="none"
                aria-hidden="true"
              >
                <path
                  d="M 4 10 C 48 10, 68 18, 110 18"
                  stroke="var(--border-strong)"
                  stroke-width="1.25"
                />
                <path
                  d="M 4 26 C 48 26, 68 18, 110 18"
                  stroke="var(--border-strong)"
                  stroke-width="1.25"
                />
                <path
                  d="M 130 18 C 172 18, 192 10, 236 10"
                  stroke="var(--border-strong)"
                  stroke-width="1.25"
                />
                <path
                  d="M 130 18 C 172 18, 192 26, 236 26"
                  stroke="var(--border-strong)"
                  stroke-width="1.25"
                />
                <Show when={app().status().ready}>
                  <path
                    class="signal-flow-line"
                    d="M 4 10 C 48 10, 68 18, 110 18"
                    stroke="var(--ink-secondary)"
                    stroke-width="1.5"
                    stroke-dasharray="4 6"
                  />
                  <path
                    class="signal-flow-line"
                    d="M 130 18 C 172 18, 192 10, 236 10"
                    stroke="var(--signal-cobalt)"
                    stroke-width="1.6"
                    stroke-dasharray="5 6"
                  />
                  <path
                    class="signal-flow-line"
                    d="M 130 18 C 172 18, 192 26, 236 26"
                    stroke="var(--signal-emerald)"
                    stroke-width="1.5"
                    stroke-dasharray="4 7"
                  />
                </Show>
                <circle
                  cx="120"
                  cy="18"
                  r="7"
                  fill="var(--bg-surface)"
                  stroke="var(--border-strong)"
                  stroke-width="1.25"
                />
                <circle
                  cx="120"
                  cy="18"
                  r="3"
                  fill={app().status().ready ? 'var(--signal-emerald)' : 'var(--ink-muted)'}
                />
              </svg>

              <div class="signal-throughput num">
                <span>↓ {formatSpeed(downSpeed())}</span>
                <span style={{ color: 'var(--ink-muted)' }}>·</span>
                <span>↑ {formatSpeed(upSpeed())}</span>
              </div>
            </div>

            <div style={{ display: 'flex', 'flex-direction': 'column', gap: '6px' }}>
              <div class="signal-split-bar" aria-hidden="true">
                <div
                  class="signal-split-overseas"
                  style={{
                    width: totalConnections() > 0 ? `${overseasRatioPct()}%` : '0%',
                  }}
                />
                <div
                  class="signal-split-domestic"
                  style={{
                    width: totalConnections() > 0 ? `${100 - overseasRatioPct()}%` : '0%',
                  }}
                />
              </div>

              <button
                type="button"
                class="signal-split-meta"
                onClick={() => app().setActiveNav('pulse')}
                style={{
                  width: '100%',
                  padding: '2px 0',
                  border: 'none',
                  background: 'transparent',
                  cursor: 'pointer',
                  font: 'inherit',
                }}
              >
                <span class="status-inline num">
                  <span class="status-dot cobalt" aria-hidden="true" />
                  <span>海外通道 {splitStats().proxy.count} 连接</span>
                </span>
                <span class="status-inline num">
                  <span class="status-dot emerald" aria-hidden="true" />
                  <span>国内直连 {splitStats().direct.count} 连接</span>
                </span>
              </button>
            </div>
          </div>

          {/* Stage 3: Outbound Exit */}
          <div class="signal-stage">
            <div class="signal-stage-head">
              <span class="status-inline">
                <span
                  class={classNames(
                    'status-dot',
                    app().status().ready && proxyNodes().length > 0 ? 'cobalt' : 'muted',
                  )}
                  aria-hidden="true"
                />
                <span>公共池主出口</span>
              </span>
              <span class="mono-tag num">{app().nodes().length} 条独享 IP</span>
            </div>

            <Show
              when={app().status().ready && proxyNodes().length > 0}
              fallback={
                <div class="row-sub" style={{ margin: 0, 'font-size': '13px' }}>
                  {app().status().ready ? '暂无公共池候选节点' : '转发引擎未启动'}
                </div>
              }
            >
              <Show
                when={isManualExit()}
                fallback={
                  <div style={{ display: 'flex', 'flex-direction': 'column', gap: '4px', 'min-width': 0 }}>
                    <div
                      style={{
                        display: 'flex',
                        'align-items': 'center',
                        gap: '6px',
                        'font-size': '13.5px',
                        'font-weight': '600',
                        color: 'var(--ink)',
                        'min-width': 0,
                      }}
                    >
                      <Show when={currentNodeDisplay().code}>
                        {(code) => <span class="mono-tag">{code()}</span>}
                      </Show>
                      <span
                        style={{
                          overflow: 'hidden',
                          'text-overflow': 'ellipsis',
                          'white-space': 'nowrap',
                        }}
                      >
                        {currentNodeDisplay().name || '自动测速选路中'}
                      </span>
                      <Show when={currentNodeName() && app().delays()[currentNodeName()] !== undefined}>
                        <span class="mono-tag num">
                          {formatDelay(app().delays()[currentNodeName()])}
                        </span>
                      </Show>
                    </div>
                    <div class="row-sub" style={{ margin: 0 }}>
                      {requestedSelect() !== 'manual'
                        ? AUTO_SELECT_LABELS[requestedSelect() as Exclude<NodeSelect, 'manual'>]
                        : '自动选路'}
                    </div>
                  </div>
                }
              >
                <SelectPopover
                  ariaLabel="切换公共池主出口"
                  value={currentNodeName()}
                  options={primaryExitOptions()}
                  onChange={(val) => {
                    void app().handleSwitchProxy(app().primaryGroupName() || 'proxy', val)
                  }}
                  disabled={Boolean(app().switchingNode())}
                  searchPlaceholder="搜索节点或地区…"
                  align="right"
                  size="sm"
                  fullWidth
                />
              </Show>
            </Show>

            <div class="signal-stage-foot">
              <span class="num">
                {proxyNodes().length > 0 ? `${proxyNodes().length} 个可用节点` : `${app().subs().length} 个订阅源`}
              </span>
              <button
                type="button"
                class="btn-link"
                onClick={() => app().setActiveNav('routes')}
              >
                管理线路 →
              </button>
            </div>
          </div>
        </div>
      </section>

      {/* Dual Clean Ledgers */}
      <div class="studio-dual-grid">
        {/* Left Ledger: Active Devices & Store Ports */}
        <section class="surface-box">
          <div class="surface-header">
            <h2 class="surface-title">在线设备与端口</h2>
            <div class="surface-actions">
              <button
                type="button"
                class="btn-link"
                onClick={() => app().setActiveNav('endpoints')}
              >
                全部 ({terminalRows().length + accessPortRows().length}) →
              </button>
            </div>
          </div>

          <Show
            when={ledgerRows().length > 0}
            fallback={
              <div class="empty-state empty-block">
                暂无在线设备或店铺端口
              </div>
            }
          >
            <div class="hairline-list">
              <For each={ledgerRows()} keyed={(item) => item.key}>
                {(item) => (
                  <div class={classNames('hairline-row', item().danger && 'danger-tint')}>
                    <div class="row-main">
                      <div class="row-title">
                        <span>{item().title}</span>
                        <Show when={item().badge}>
                          {(tag) => <span class="mono-tag">{tag()}</span>}
                        </Show>
                        <span class="mono-tag">{item().address}</span>
                      </div>
                    </div>
                    <div class="row-end">
                      <span class={classNames('status-inline', item().danger && 'danger')}>
                        <span class={classNames('status-dot', item().dotTone)} aria-hidden="true" />
                        <Show when={item().routeCode}>
                          {(code) => <span class="mono-tag">{code()}</span>}
                        </Show>
                        <span>{item().routeLabel}</span>
                      </span>
                      <span class="mono-tag num">
                        ↓ {formatSpeed(item().downloadSpeed)}
                      </span>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </section>

        {/* Right Ledger: Active Cross-Border Platforms */}
        <section class="surface-box">
          <div class="surface-header">
            <h2 class="surface-title">活跃跨境业务</h2>
            <div class="surface-actions">
              <button
                type="button"
                class="btn-link"
                onClick={() => app().setActiveNav('pulse')}
              >
                分流与脉冲 →
              </button>
            </div>
          </div>

          <Show
            when={activePlatforms().length > 0}
            fallback={
              <div class="empty-state empty-block">
                当前暂无活跃跨境业务连接
              </div>
            }
          >
            <div class="hairline-list">
              <For each={activePlatforms()} keyed={(item) => item.platform.id}>
                {(item) => (
                  <div class="hairline-row">
                    <div class="row-main row-lead">
                      <span
                        class="app-mark"
                        style={{ background: item().platform.accent }}
                        aria-hidden="true"
                      >
                        {item().platform.shortLabel}
                      </span>
                      <div style={{ 'min-width': 0 }}>
                        <div class="row-title">{item().platform.name}</div>
                        <div class="row-sub">
                          {item().outbounds.slice(0, 2).map(formatOutboundCompact).join(' · ')}
                        </div>
                      </div>
                    </div>
                    <div class="row-end num" style={{ 'flex-direction': 'column', 'align-items': 'flex-end', gap: '2px' }}>
                      <span style={{ 'font-size': '12.5px', 'font-weight': '600', color: 'var(--ink)' }}>
                        ↓ {formatSpeed(item().downloadSpeed)}
                      </span>
                      <span class="row-sub" style={{ margin: 0 }}>
                        {item().connectionCount} 连接
                      </span>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </section>
      </div>
    </>
  )
}
