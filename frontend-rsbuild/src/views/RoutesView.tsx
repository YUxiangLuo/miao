import { For, Show, createMemo, createSignal } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import { buildProxyRegionBuckets, classifyProxyRegion, type ProxyRegionId } from '../proxyRegions'
import {
  classNames,
  formatDelay,
  getDelayTone,
  maskSubscription,
} from '../utils'
import { buildDedicatedLines } from '../domain/lines'
import type { NodeSelect, SubStatus, VergeImportItem } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import {
  ClockIcon,
  DownloadIcon,
  LockIcon,
  PlusIcon,
  RefreshCwIcon,
  SearchIcon,
  Trash2Icon,
  XIcon,
  ZapIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export interface RoutesViewProps {
  app: AppController
  onInspectSub: (sub: SubStatus) => void
  onOpenSchedule: () => void
}

const NODE_SELECT_OPTIONS: ReadonlyArray<{ value: NodeSelect; label: string }> = [
  { value: 'manual', label: '手动选路' },
  { value: 'fastest_hk', label: '自动 · HK' },
  { value: 'fastest_jp', label: '自动 · JP' },
  { value: 'fastest_sg', label: '自动 · SG' },
  { value: 'fastest_us', label: '自动 · US' },
  { value: 'fastest_tw', label: '自动 · TW' },
]

const REGION_CODE_LABELS: Record<ProxyRegionId, string> = {
  all: '全部',
  hk: 'HK',
  jp: 'JP',
  sg: 'SG',
  us: 'US',
  tw: 'TW',
  eu: 'EU',
  other: '其他',
}

function formatProtocolTag(nodeType: string): string {
  switch (nodeType.toLowerCase()) {
    case 'socks':
      return 'SOCKS5'
    case 'hysteria2':
      return 'HY2'
    case 'ss':
    case 'shadowsocks':
      return 'SS'
    case 'vless':
      return 'VLESS'
    case 'vmess':
      return 'VMESS'
    case 'trojan':
      return 'TROJAN'
    case 'anytls':
      return 'ANYTLS'
    case 'tuic':
      return 'TUIC'
    default:
      return nodeType.toUpperCase()
  }
}

export function RoutesView(props: RoutesViewProps) {
  const app = () => props.app

  const [subUrl, setSubUrl] = createSignal('')
  const [addingSubOpen, setAddingSubOpen] = createSignal(false)
  const [vergeItems, setVergeItems] = createSignal<VergeImportItem[] | null>(null)
  const [scanningVerge, setScanningVerge] = createSignal(false)
  const [poolRegion, setPoolRegion] = createSignal<ProxyRegionId>('all')
  const [poolQuery, setPoolQuery] = createSignal('')

  const dedicatedLines = createMemo(() => buildDedicatedLines({
    nodes: app().nodes(),
    rules: app().rules(),
    accessPorts: app().accessPorts(),
    availableTargetNames: app().ruleNodeNames(),
    delays: app().delays(),
    testingNodes: app().testingNodes(),
  }))

  const requestedSelect = createMemo<NodeSelect>(
    () => app().status().requested_node_select || app().status().node_select || 'manual',
  )
  const candidateNodes = createMemo(() => app().primaryGroup()?.all || [])
  const currentSelectedNode = createMemo(() => app().primaryGroup()?.now || '')
  const regionBuckets = createMemo(() => buildProxyRegionBuckets(candidateNodes()))

  const multiplierOptions = createMemo<SelectOption[]>(() => [
    { value: '', label: '不限倍率' },
    ...app().status().multiplier_options.map((mult) => ({
      value: mult,
      label: `最高 ${mult}x`,
    })),
  ])

  const visiblePoolNodes = createMemo(() => {
    const needle = poolQuery().trim().toLowerCase()
    const region = poolRegion()
    const active = currentSelectedNode()

    const filtered = candidateNodes().filter((rawName) => {
      if (region !== 'all' && classifyProxyRegion(rawName) !== region) return false
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

  const submitSubscription = async (event?: Event) => {
    event?.preventDefault()
    if (!subUrl().trim()) return
    const ok = await app().handleAddSubscription(subUrl())
    if (ok) {
      setSubUrl('')
      setAddingSubOpen(false)
    }
  }

  const scanVerge = async () => {
    if (scanningVerge()) return
    setScanningVerge(true)
    try {
      const result = await app().scanClashVerge()
      if (!result || !result.found || result.items.length === 0) {
        app().showToast('未在本机检测到可导入的 Clash Verge 订阅源', 'info')
        return
      }
      const unadded = result.items.filter((item) => !item.already_added)
      if (unadded.length === 0) {
        app().showToast('本机 Clash Verge 订阅源均已添加', 'info')
        return
      }
      setVergeItems(result.items)
    } finally {
      setScanningVerge(false)
    }
  }

  const importAvailableVerge = async () => {
    const urls = (vergeItems() || []).filter((item) => !item.already_added).map((item) => item.url)
    if (urls.length === 0) {
      setVergeItems(null)
      return
    }
    if (await app().importClashVergeSubs(urls)) {
      setVergeItems(null)
      setAddingSubOpen(false)
    }
  }

  return (
    <>
      {/* Section 1: Dedicated Static IP Assets */}
      <section class="surface-box" aria-label="独享静态 IP">
        <div class="surface-header">
          <h2 class="surface-title">
            <span>独享静态 IP</span>
            <span class="mono-tag num">{dedicatedLines().length}</span>
          </h2>
          <div class="surface-actions">
            <Button
              tone="secondary"
              size="sm"
              icon={<LockIcon size={ICON.xs} />}
              onClick={() => app().openNodeModal('socks-residential')}
            >
              录入住宅 SOCKS5
            </Button>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={() => app().openNodeModal('vps')}
            >
              部署 VPS / 导入
            </Button>
          </div>
        </div>

        <Show
          when={dedicatedLines().length > 0}
          fallback={
            <div class="empty-state empty-block">
              尚未录入独享静态 IP。可录入海外静态住宅 SOCKS5 或部署独享云服务器。
            </div>
          }
        >
          <div class="hairline-list">
            <For each={dedicatedLines()} keyed={(row) => row.node.tag}>
              {(row) => {
                const cleaned = () => cleanNodeDisplay(row().node.tag)
                const tone = () => getDelayTone(row().delay)
                return (
                  <div class={classNames('hairline-row', !row().relayExists && 'danger-tint')}>
                    <div class="row-main">
                      <div class="row-title">
                        <span
                          class={classNames(
                            'status-dot',
                            !row().relayExists
                              ? 'crimson'
                              : tone() === 'timeout'
                                ? 'amber'
                                : row().isStaticResidential
                                  ? 'emerald'
                                  : 'cobalt',
                          )}
                        />
                        <Show when={cleaned().code}>
                          {(code) => <span class="mono-tag">{code()}</span>}
                        </Show>
                        <span>{cleaned().name}</span>
                        <span class="mono-tag">{formatProtocolTag(row().node.node_type)}</span>
                        <Show when={row().boundSeatCount > 0}>
                          <span class="mono-tag num">{row().boundSeatCount} 台设备</span>
                        </Show>
                        <Show when={row().boundPortCount > 0}>
                          <span class="mono-tag num">{row().boundPortCount} 个店铺端口</span>
                        </Show>
                      </div>
                      <div
                        class="row-sub mono"
                        style={{ display: 'flex', 'align-items': 'center', gap: '8px', 'flex-wrap': 'wrap' }}
                      >
                        <span>
                          {row().node.server}:{row().node.server_port}
                          {row().node.sni ? ` · ${row().node.sni}` : ''}
                        </span>
                        <Show when={row().detourRelay}>
                          {(detour) => (
                            <span class={classNames('chain-pill', !row().relayExists && 'danger')}>
                              {cleanNodeDisplay(detour()).name} ──▶ {cleaned().name}
                              {!row().relayExists ? '（前置缺失）' : ''}
                            </span>
                          )}
                        </Show>
                      </div>
                    </div>

                    <div class="row-end">
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
                        {row().testing
                          ? '测速中'
                          : row().delay == null
                            ? '测速'
                            : formatDelay(row().delay)}
                      </span>
                      <button
                        type="button"
                        class="icon-button"
                        aria-label={`测速 ${cleaned().name}`}
                        title="节点测速"
                        disabled={!app().status().ready || row().testing}
                        onClick={() => app().handleTestDelay(row().node.tag)}
                      >
                        <ZapIcon size={ICON.sm} />
                      </button>
                      <button
                        type="button"
                        class="icon-button danger"
                        aria-label={`删除专线 ${row().node.tag}`}
                        title="删除独享静态 IP"
                        onClick={() => app().handleOpenDeleteNodeConfirm(row().node.tag)}
                      >
                        <Trash2Icon size={ICON.sm} />
                      </button>
                    </div>
                  </div>
                )
              }}
            </For>
          </div>
        </Show>
      </section>

      {/* Section 2: Shared Outbound Subscription Pool */}
      <section class="surface-box" aria-label="公共出海池">
        <div class="surface-header">
          <h2 class="surface-title">
            <span>公共出海池</span>
            <span class="mono-tag num">{app().subs().length}</span>
          </h2>
          <div class="surface-actions">
            <Button
              tone="secondary"
              size="sm"
              icon={<ClockIcon size={ICON.xs} />}
              onClick={props.onOpenSchedule}
            >
              定时更新
            </Button>
            <Button
              tone="secondary"
              size="sm"
              icon={<RefreshCwIcon size={ICON.xs} />}
              loading={app().pendingActions().has('refreshSubs')}
              disabled={app().subs().length === 0}
              onClick={app().handleOpenRefreshSubscriptionsConfirm}
            >
              刷新订阅
            </Button>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={() => setAddingSubOpen((prev) => !prev)}
            >
              添加订阅
            </Button>
          </div>
        </div>

        <Show when={app().subs().length > 0}>
          <div class="sub-strip" role="list" aria-label="订阅源列表">
            <For each={app().subs()} keyed={(sub) => sub.url}>
              {(sub) => {
                const enabledCount = () => Math.max(0, sub().node_count - sub().disabled_count)
                const dotTone = () => (
                  sub().state === 'ready'
                    ? 'emerald'
                    : sub().state === 'refreshing'
                      ? 'cobalt pulse'
                      : sub().state === 'failed'
                        ? 'crimson'
                        : 'amber'
                )
                return (
                  <div class="sub-chip" role="listitem">
                    <button
                      type="button"
                      onClick={() => props.onInspectSub(sub())}
                      title={`${sub().url} · 点击管理节点启停`}
                      style={{
                        display: 'inline-flex',
                        'align-items': 'center',
                        gap: '7px',
                        border: 'none',
                        background: 'transparent',
                        padding: '0',
                        color: 'inherit',
                        font: 'inherit',
                        cursor: 'pointer',
                        'min-width': '0',
                      }}
                    >
                      <span class={classNames('status-dot', dotTone())} />
                      <span class="mono">{maskSubscription(sub().url)}</span>
                      <span class="mono-tag num">
                        {sub().disabled_count > 0
                          ? `${enabledCount()}/${sub().node_count}`
                          : `${sub().node_count}`}
                      </span>
                    </button>
                    <button
                      type="button"
                      class="icon-button danger"
                      style={{ width: '22px', height: '22px' }}
                      aria-label={`删除订阅 ${maskSubscription(sub().url)}`}
                      title="删除订阅源"
                      onClick={() => app().handleOpenDeleteSubConfirm(sub().url)}
                    >
                      <XIcon size={12} />
                    </button>
                  </div>
                )
              }}
            </For>
          </div>
        </Show>

        <div class="surface-body" style={{ display: 'flex', 'flex-direction': 'column', gap: '16px' }}>
          <Show when={addingSubOpen()}>
            <form
              onSubmit={(e) => { void submitSubscription(e) }}
              style={{
                display: 'flex',
                'align-items': 'center',
                gap: '8px',
                'flex-wrap': 'wrap',
                padding: '10px 12px',
                'border-radius': '10px',
                background: 'var(--bg-subtle)',
                border: '1px solid var(--hairline)',
              }}
            >
              <input
                data-autofocus
                aria-label="订阅地址"
                placeholder="https://example.com/api/v1/client/subscribe?token=..."
                value={subUrl()}
                disabled={app().pendingActions().has('addSub')}
                onInput={(e) => setSubUrl(e.currentTarget.value)}
                style={{
                  flex: '1',
                  'min-width': '240px',
                  height: '30px',
                  padding: '0 10px',
                  'border-radius': '7px',
                  border: '1px solid var(--border-strong)',
                  background: 'var(--bg-surface)',
                  color: 'var(--ink)',
                  'font-size': '12.5px',
                }}
              />
              <Button
                type="submit"
                tone="primary"
                size="sm"
                loading={app().pendingActions().has('addSub')}
              >
                保存订阅
              </Button>
              <Button
                tone="secondary"
                size="sm"
                icon={<DownloadIcon size={ICON.xs} />}
                loading={scanningVerge() || app().pendingActions().has('importVerge')}
                onClick={() => { void scanVerge() }}
              >
                从 Clash Verge 导入
              </Button>
              <Button tone="ghost" size="sm" onClick={() => setAddingSubOpen(false)}>
                取消
              </Button>
            </form>
          </Show>

          <Show when={vergeItems()}>
            {(items) => (
              <div class="circuit-banner">
                <span>
                  检测到 {items().length} 个本机 Clash Verge 订阅源（{items().filter((i) => !i.already_added).length} 个待导入）
                </span>
                <div style={{ display: 'flex', 'align-items': 'center', gap: '8px' }}>
                  <Button
                    tone="primary"
                    size="sm"
                    loading={app().pendingActions().has('importVerge')}
                    onClick={() => { void importAvailableVerge() }}
                  >
                    确认导入
                  </Button>
                  <Button tone="ghost" size="sm" onClick={() => setVergeItems(null)}>
                    取消
                  </Button>
                </div>
              </div>
            )}
          </Show>

          <div class="studio-toolbar">
            <div class="seg-rail" role="group" aria-label="公共池选路策略">
              <For each={NODE_SELECT_OPTIONS}>
                {(opt) => (
                  <button
                    type="button"
                    class={classNames('seg-btn', requestedSelect() === opt.value && 'active')}
                    aria-pressed={requestedSelect() === opt.value ? 'true' : 'false'}
                    disabled={app().pendingActions().has('nodeSelect')}
                    onClick={() => { void app().handleSetNodeSelect(opt.value) }}
                  >
                    {opt.label}
                  </button>
                )}
              </For>
            </div>

            <div style={{ display: 'flex', 'align-items': 'center', gap: '8px', 'flex-wrap': 'wrap' }}>
              <Show when={requestedSelect() !== 'manual' && app().status().multiplier_options.length > 0}>
                <SelectPopover
                  ariaLabel="最高倍率限制"
                  size="sm"
                  align="right"
                  value={app().status().max_multiplier ?? ''}
                  options={multiplierOptions()}
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
            </div>
          </div>

          <Show
            when={candidateNodes().length > 0}
            fallback={
              <Show when={app().subs().length === 0}>
                <div class="empty-state empty-block" style={{ padding: '18px 12px' }}>
                  尚未添加订阅源。点击右上角「添加订阅」拉取公共池出口节点。
                </div>
              </Show>
            }
          >
            <div class="studio-toolbar">
              <div class="seg-rail" role="group" aria-label="地区筛选">
                <For each={regionBuckets()}>
                  {(bucket) => (
                    <button
                      type="button"
                      class={classNames('seg-btn', poolRegion() === bucket.id && 'active')}
                      aria-pressed={poolRegion() === bucket.id ? 'true' : 'false'}
                      onClick={() => setPoolRegion(bucket.id)}
                    >
                      {REGION_CODE_LABELS[bucket.id]} <span class="num">{bucket.count}</span>
                    </button>
                  )}
                </For>
              </div>

              <div class="studio-search">
                <SearchIcon size={ICON.sm} />
                <input
                  type="search"
                  aria-label="搜索公共池节点"
                  placeholder="搜索节点或地区…"
                  value={poolQuery()}
                  onInput={(e) => setPoolQuery(e.currentTarget.value)}
                />
              </div>
            </div>

            <Show
              when={visiblePoolNodes().length > 0}
              fallback={
                <div class="empty-state empty-block" style={{ padding: '20px 12px' }}>
                  没有匹配当前筛选条件的节点。
                </div>
              }
            >
              <div class="node-matrix" role="group" aria-label="公共池候选节点">
                <For each={visiblePoolNodes()}>
                  {(nodeName) => {
                    const selected = () => currentSelectedNode() === nodeName
                    const cleaned = () => cleanNodeDisplay(nodeName)
                    const delay = () => app().delays()[nodeName]
                    const tone = () => getDelayTone(delay())
                    return (
                      <button
                        type="button"
                        class={classNames('node-cell', selected() && 'selected')}
                        aria-pressed={selected() ? 'true' : 'false'}
                        disabled={requestedSelect() !== 'manual' || Boolean(app().switchingNode())}
                        title={
                          requestedSelect() !== 'manual'
                            ? `${cleaned().name}（切换到「手动选路」后可点选）`
                            : `切换主出口为 ${cleaned().name}`
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
        </div>
      </section>
    </>
  )
}
