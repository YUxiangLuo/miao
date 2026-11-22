import { For, Show, createMemo, createSignal } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { ConnectivityCheck } from '../components/ConnectivityCheck'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import { splitConnectionStats } from '../components/connectionFilters'
import { buildProxyRegionBuckets, classifyProxyRegion, type ProxyRegionId } from '../proxyRegions'
import {
  classNames,
  formatDelay,
  formatSpeed,
  formatUptime,
  getDelayTone,
} from '../utils'
import { buildTerminalRows } from '../domain/terminals'
import { buildAccessPortRows } from '../domain/accessPorts'
import type { NodeSelect, RouteMode } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import { PlayIcon, PlusIcon, SearchIcon, SquareIcon, ZapIcon } from '../components/icons'

type AppController = ReturnType<typeof useAppController>

const STRATEGY_OPTIONS: SelectOption[] = [
  { value: 'manual', label: '手动选择' },
  { value: 'fastest_hk', label: '自动优选 · 香港 (HK)' },
  { value: 'fastest_jp', label: '自动优选 · 日本 (JP)' },
  { value: 'fastest_sg', label: '自动优选 · 新加坡 (SG)' },
  { value: 'fastest_us', label: '自动优选 · 美国 (US)' },
  { value: 'fastest_tw', label: '自动优选 · 台湾 (TW)' },
]

const REGION_LABELS: Record<ProxyRegionId, string> = {
  all: '全部',
  hk: 'HK',
  jp: 'JP',
  sg: 'SG',
  us: 'US',
  tw: 'TW',
  eu: 'EU',
  other: '其他',
}

export function OverviewView(props: { app: AppController }): JSX.Element {
  const app = () => props.app
  const [region, setRegion] = createSignal<ProxyRegionId>('all')
  const [query, setQuery] = createSignal('')

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

  const blockedDeviceCount = createMemo(() => terminalRows().filter((row) => row.failClosed).length)
  const blockedPortCount = createMemo(() => accessPortRows().filter((row) => row.blocked).length)
  const blockedTotal = createMemo(() => blockedDeviceCount() + blockedPortCount())

  const downSpeed = createMemo(
    () => app().traffic().down ?? (splitStats().proxy.downloadSpeed + splitStats().direct.downloadSpeed),
  )
  const upSpeed = createMemo(
    () => app().traffic().up ?? (splitStats().proxy.uploadSpeed + splitStats().direct.uploadSpeed),
  )

  const candidateNodes = createMemo(() => app().primaryGroup()?.all ?? [])
  const currentNode = createMemo(() => app().primaryGroup()?.now ?? '')
  const currentDisplay = createMemo(() => cleanNodeDisplay(currentNode()))
  const requestedSelect = createMemo<NodeSelect>(
    () => app().status().requested_node_select || app().status().node_select || 'manual',
  )
  const isManual = createMemo(
    () => requestedSelect() === 'manual' && app().status().node_select === 'manual',
  )

  const regionBuckets = createMemo(() => buildProxyRegionBuckets(candidateNodes()))

  const multiplierOptions = createMemo<SelectOption[]>(() => [
    { value: '', label: '不限倍率' },
    ...app().status().multiplier_options.map((mult) => ({
      value: mult,
      label: `≤ ${mult}x`,
    })),
  ])

  const visibleNodes = createMemo(() => {
    const needle = query().trim().toLowerCase()
    const reg = region()
    const active = currentNode()

    const filtered = candidateNodes().filter((rawName) => {
      if (reg !== 'all' && classifyProxyRegion(rawName) !== reg) return false
      if (!needle) return true
      const cleaned = cleanNodeDisplay(rawName)
      return (
        rawName.toLowerCase().includes(needle)
        || cleaned.name.toLowerCase().includes(needle)
        || (cleaned.code !== null && cleaned.code.toLowerCase().includes(needle))
      )
    })

    if (!active) return filtered
    const activeIndex = filtered.indexOf(active)
    if (activeIndex <= 0) return filtered
    return [active, ...filtered.slice(0, activeIndex), ...filtered.slice(activeIndex + 1)]
  })

  const switchRouteMode = (mode: RouteMode) => {
    if (mode === app().status().route_mode) return
    app().handleOpenSetRouteModeConfirm(mode)
  }

  const totalSubNodes = createMemo(() => (
    app().subs().reduce((sum, s) => sum + Math.max(0, s.node_count - s.disabled_count), 0)
  ))

  return (
    <>
      <header class="page-header">
        <div>
          <h1 class="page-title">概览</h1>
          <p class="page-subtitle">管理服务状态、分流模式，并快速测速切换默认代理出口</p>
        </div>
        <div class="page-actions">
          <ConnectivityCheck
            status={app().status()}
            backendUnreachable={app().backendUnreachable()}
          />
          <Button
            tone="primary"
            size="sm"
            icon={<PlusIcon size={ICON.xs} />}
            onClick={() => app().openNodeModal('link')}
          >
            添加节点
          </Button>
        </div>
      </header>

      <Show when={blockedTotal() > 0}>
        <div class="circuit-banner danger" role="alert">
          <div class="circuit-banner-main">
            <span class="status-dot crimson pulse" aria-hidden="true" />
            <span>
              有 {blockedTotal()} 项设备或独立端口所绑定的节点当前不可用，已自动阻断连接以防意外回退
            </span>
          </div>
          <div class="circuit-banner-actions">
            <Button tone="secondary" size="sm" onClick={() => app().setActiveNav('lan')}>
              查看设备与端口
            </Button>
            <Button tone="ghost" size="sm" onClick={() => app().setActiveNav('nodes')}>
              检查节点
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

      {/* Top 3 Control & Telemetry Cards */}
      <section class="overview-cards" aria-label="核心控制卡片">
        {/* Card 1: Service Status */}
        <div class="surface-box overview-card">
          <div class="overview-card-head">
            <span class="overview-card-label">代理服务</span>
            <Show when={app().status().platform}>
              {(plat) => <span class="mono-tag">{plat()}</span>}
            </Show>
          </div>
          <div class="overview-card-body">
            <div class="overview-stat-line">
              <span
                class={classNames(
                  'status-dot',
                  app().status().ready ? 'emerald pulse' : app().status().initializing ? 'amber' : 'muted',
                )}
                aria-hidden="true"
              />
              <strong class="overview-stat-value">
                {app().status().ready ? '运行中' : app().status().initializing ? '正在启动' : '已停止'}
              </strong>
            </div>
            <Show
              when={app().status().running || app().status().ready}
              fallback={
                <Button
                  tone="primary"
                  size="sm"
                  icon={<PlayIcon size={ICON.xs} />}
                  loading={app().pendingActions().has('startService')}
                  onClick={() => { void app().handleStartService() }}
                >
                  启动服务
                </Button>
              }
            >
              <Button
                tone="secondary"
                size="sm"
                icon={<SquareIcon size={ICON.xs} />}
                loading={app().pendingActions().has('stopService')}
                onClick={() => app().handleOpenStopServiceConfirm()}
              >
                停止服务
              </Button>
            </Show>
          </div>
          <div class="overview-card-foot num">
            {app().status().ready
              ? `运行时长 ${formatUptime(app().status().uptime_secs)}`
              : '透明代理数据面当前未启用'}
          </div>
        </div>

        {/* Card 2: Route Mode */}
        <div class="surface-box overview-card">
          <div class="overview-card-head">
            <span class="overview-card-label">分流模式</span>
          </div>
          <div class="overview-card-body">
            <div class="seg-rail full" role="group" aria-label="分流模式">
              <button
                type="button"
                class={classNames('seg-btn', app().status().route_mode === 'rule' && 'active')}
                aria-pressed={app().status().route_mode === 'rule' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('rule')}
              >
                规则分流
              </button>
              <button
                type="button"
                class={classNames('seg-btn', app().status().route_mode === 'global' && 'active')}
                aria-pressed={app().status().route_mode === 'global' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('global')}
              >
                全局代理
              </button>
            </div>
          </div>
          <div class="overview-card-foot">
            {app().status().route_mode === 'rule'
              ? '内网与大陆流量直连，其余按规则走代理'
              : '除直连规则外，全部流量走代理出口'}
          </div>
        </div>

        {/* Card 3: Live Throughput */}
        <div class="surface-box overview-card">
          <div class="overview-card-head">
            <span class="overview-card-label">实时吞吐</span>
            <button
              type="button"
              class="btn-link"
              onClick={() => app().setActiveNav('connections')}
            >
              查看连接 →
            </button>
          </div>
          <div class="overview-card-body">
            <div class="overview-rates num">
              <span>↓ {formatSpeed(downSpeed())}</span>
              <span class="overview-rate-up">↑ {formatSpeed(upSpeed())}</span>
            </div>
          </div>
          <div class="overview-card-foot num">
            <span>代理 {splitStats().proxy.count} 连接</span>
            <span>·</span>
            <span>直连 {splitStats().direct.count} 连接</span>
          </div>
        </div>
      </section>

      {/* Primary Node Switcher Surface */}
      <section class="surface-box" aria-label="默认代理出口">
        <div class="surface-header">
          <div class="surface-title-group">
            <h2 class="surface-title">默认代理出口</h2>
            <Show when={app().status().ready && currentNode()}>
              <span class="current-node-pill">
                <span class="status-dot emerald" aria-hidden="true" />
                <Show when={currentDisplay().code}>
                  {(code) => <span class="mono-tag">{code()}</span>}
                </Show>
                <span class="truncate">{currentDisplay().name}</span>
                <Show when={app().delays()[currentNode()] != null}>
                  <span class="mono-tag num">{formatDelay(app().delays()[currentNode()])}</span>
                </Show>
              </span>
            </Show>
          </div>

          <div class="surface-actions">
            <div class="strategy-picker-wrap">
              <SelectPopover
                ariaLabel="选路方式"
                size="sm"
                align="right"
                value={requestedSelect()}
                options={STRATEGY_OPTIONS}
                disabled={app().pendingActions().has('nodeSelect')}
                searchableThreshold={10}
                onChange={(val) => {
                  void app().handleSetNodeSelect(val as NodeSelect)
                }}
              />
            </div>

            <Show when={requestedSelect() !== 'manual' && app().status().multiplier_options.length > 0}>
              <SelectPopover
                ariaLabel="最高倍率限制"
                size="sm"
                align="right"
                value={app().status().max_multiplier ?? ''}
                options={multiplierOptions()}
                searchableThreshold={12}
                onChange={(val) => {
                  void app().handleSetMaxMultiplier(val === '' ? null : val)
                }}
              />
            </Show>

            <Show when={app().status().ready && candidateNodes().length > 0}>
              <Button
                tone="secondary"
                size="sm"
                icon={<ZapIcon size={ICON.xs} />}
                loading={Boolean(app().testingGroup())}
                onClick={() => app().handleTestGroupDelays(app().primaryGroupName() || 'proxy', candidateNodes())}
              >
                批量测速
              </Button>
            </Show>

            <button
              type="button"
              class="btn-link"
              onClick={() => app().setActiveNav('nodes')}
            >
              管理节点 →
            </button>
          </div>
        </div>

        <Show when={!isManual() && app().status().ready && candidateNodes().length > 0}>
          <div class="surface-note">
            <span>
              当前处于自动测速选路模式，系统会在所选地区节点中自动维持最低延迟出口。如需手动指定节点，请切换为手动选择。
            </span>
            <Button
              tone="secondary"
              size="sm"
              disabled={app().pendingActions().has('nodeSelect')}
              onClick={() => { void app().handleSetNodeSelect('manual') }}
            >
              切回手动选择
            </Button>
          </div>
        </Show>

        <div class="surface-body">
          <Show
            when={app().status().ready}
            fallback={
              <div class="empty-state">
                代理服务当前未运行，启动服务后即可查看实时节点池并测速切换。
              </div>
            }
          >
            <Show
              when={candidateNodes().length > 0}
              fallback={
                <div class="empty-state">
                  当前没有可用的候选节点，请前往「节点与订阅」添加订阅链接或手动节点。
                </div>
              }
            >
              <Show when={candidateNodes().length > 6}>
                <div class="studio-toolbar" style={{ 'margin-bottom': '14px' }}>
                  <div class="seg-rail" role="group" aria-label="地区筛选">
                    <For each={regionBuckets()}>
                      {(bucket) => (
                        <button
                          type="button"
                          class={classNames('seg-btn', region() === bucket.id && 'active')}
                          aria-pressed={region() === bucket.id ? 'true' : 'false'}
                          onClick={() => setRegion(bucket.id)}
                        >
                          {REGION_LABELS[bucket.id]} <span class="num">{bucket.count}</span>
                        </button>
                      )}
                    </For>
                  </div>

                  <Show when={candidateNodes().length > 10}>
                    <div class="studio-search">
                      <SearchIcon size={ICON.sm} />
                      <input
                        type="search"
                        aria-label="搜索候选节点"
                        placeholder="搜索节点或地区…"
                        value={query()}
                        onInput={(e) => setQuery(e.currentTarget.value)}
                      />
                    </div>
                  </Show>
                </div>
              </Show>

              <Show
                when={visibleNodes().length > 0}
                fallback={<div class="empty-state">没有匹配当前筛选条件的节点。</div>}
              >
                <div class="node-matrix" role="group" aria-label="候选节点列表">
                  <For each={visibleNodes()}>
                    {(nodeName) => {
                      const selected = () => currentNode() === nodeName
                      const cleaned = () => cleanNodeDisplay(nodeName)
                      const delay = () => app().delays()[nodeName]
                      const tone = () => getDelayTone(delay())
                      return (
                        <button
                          type="button"
                          class={classNames('node-cell', selected() && 'selected')}
                          aria-pressed={selected() ? 'true' : 'false'}
                          disabled={!isManual() || Boolean(app().switchingNode())}
                          title={
                            !isManual()
                              ? `${cleaned().name}（切回「手动选择」后可手动指定）`
                              : `切换默认出口为 ${cleaned().name}`
                          }
                          onClick={() => {
                            void app().handleSwitchProxy(app().primaryGroupName() || 'proxy', nodeName)
                          }}
                        >
                          <span class="node-cell-main">
                            <span
                              class={classNames(
                                'status-dot',
                                selected()
                                  ? 'emerald'
                                  : tone() === 'timeout'
                                    ? 'crimson'
                                    : 'muted',
                              )}
                            />
                            <Show when={cleaned().code}>
                              {(code) => <span class="mono-tag">{code()}</span>}
                            </Show>
                            <span class="node-cell-name">{cleaned().name}</span>
                          </span>
                          <span
                            class={classNames(
                              'latency-text',
                              'num',
                              tone() === 'fast'
                                ? 'fast'
                                : tone() === 'medium'
                                  ? 'medium'
                                  : (tone() === 'slow' || tone() === 'timeout') && 'slow',
                            )}
                          >
                            {app().testingNodes()[nodeName]
                              ? '…'
                              : delay() == null
                                ? '—'
                                : formatDelay(delay())}
                          </span>
                        </button>
                      )
                    }}
                  </For>
                </div>
              </Show>
            </Show>
          </Show>
        </div>
      </section>

      {/* Bottom Compact Summary Grid */}
      <div class="overview-summary-grid">
        <section class="surface-box summary-card" aria-label="配置概览">
          <div class="surface-header">
            <h2 class="surface-title">订阅与规则</h2>
            <button
              type="button"
              class="btn-link"
              onClick={() => app().setActiveNav('rules')}
            >
              分流规则 ({app().rules().length}) →
            </button>
          </div>
          <div class="summary-metrics num">
            <button
              type="button"
              class="summary-metric-item"
              onClick={() => app().setActiveNav('nodes')}
            >
              <span class="summary-metric-num">{app().subs().length}</span>
              <span class="summary-metric-label">订阅源（{totalSubNodes()} 节点）</span>
            </button>
            <button
              type="button"
              class="summary-metric-item"
              onClick={() => app().setActiveNav('nodes')}
            >
              <span class="summary-metric-num">{app().nodes().length}</span>
              <span class="summary-metric-label">手动节点</span>
            </button>
            <button
              type="button"
              class="summary-metric-item"
              onClick={() => app().setActiveNav('rules')}
            >
              <span class="summary-metric-num">{app().rules().length}</span>
              <span class="summary-metric-label">自定义规则</span>
            </button>
          </div>
        </section>

        <section class="surface-box summary-card" aria-label="局域网概览">
          <div class="surface-header">
            <h2 class="surface-title">局域网与独立端口</h2>
            <button
              type="button"
              class="btn-link"
              onClick={() => app().setActiveNav('lan')}
            >
              管理设备与端口 →
            </button>
          </div>
          <div class="summary-metrics num">
            <button
              type="button"
              class="summary-metric-item"
              onClick={() => app().setActiveNav('lan')}
            >
              <span class="summary-metric-num">{terminalRows().length}</span>
              <span class="summary-metric-label">局域网设备</span>
            </button>
            <button
              type="button"
              class="summary-metric-item"
              onClick={() => app().setActiveNav('lan')}
            >
              <span class="summary-metric-num">
                {terminalRows().filter((r) => r.mode !== 'pool').length}
              </span>
              <span class="summary-metric-label">自定义出口设备</span>
            </button>
            <button
              type="button"
              class="summary-metric-item"
              onClick={() => app().setActiveNav('lan')}
            >
              <span class="summary-metric-num">{accessPortRows().length}</span>
              <span class="summary-metric-label">独立代理端口</span>
            </button>
          </div>
        </section>
      </div>
    </>
  )
}
