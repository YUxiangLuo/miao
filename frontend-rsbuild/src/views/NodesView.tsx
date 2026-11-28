import { For, Show, createMemo, createProjection, createSignal } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import { buildProxyRegionBuckets, classifyProxyRegion, type ProxyRegionId } from '../proxyRegions'
import {
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
  ListPlusIcon,
  PlayIcon,
  PlusIcon,
  RefreshCwIcon,
  RocketIcon,
  SearchIcon,
  Trash2Icon,
  ZapIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export interface NodesViewProps {
  app: AppController
  onInspectSub: (sub: SubStatus) => void
  onOpenSchedule: () => void
}

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

function formatSubStateText(sub: SubStatus): string {
  const enabledCount = Math.max(0, sub.node_count - sub.disabled_count)
  if (sub.state === 'refreshing') return '正在拉取更新…'
  if (sub.state === 'failed') {
    return sub.error ? `拉取失败：${sub.error}` : '拉取失败'
  }
  if (sub.state === 'pending') return '等待首次拉取'
  if (sub.disabled_count > 0) {
    return `已启用 ${enabledCount} / 共 ${sub.node_count} 个节点`
  }
  return `共 ${sub.node_count} 个节点`
}

export function NodesView(props: NodesViewProps) {
  const app = () => props.app

  const [subUrl, setSubUrl] = createSignal('')
  const [addingSubOpen, setAddingSubOpen] = createSignal(false)
  const [vergeItems, setVergeItems] = createSignal<VergeImportItem[] | null>(null)
  const [scanningVerge, setScanningVerge] = createSignal(false)
  const [region, setRegion] = createSignal<ProxyRegionId>('all')
  const [query, setQuery] = createSignal('')

  const tab = () => app().nodesTab()

  const manualLines = createMemo(() => buildDedicatedLines({
    nodes: app().nodes(),
    rules: app().rules(),
    accessPorts: app().accessPorts(),
    availableTargetNames: app().ruleNodeNames(),
    delays: app().delays(),
    testingNodes: app().testingNodes(),
  }))

  const sourceTotalCount = createMemo(() => app().subs().length + manualLines().length)
  const sourceAlertCount = createMemo(() => (
    app().subs().filter((sub) => sub.state === 'failed').length
    + manualLines().filter((row) => !row.relayExists).length
  ))

  const requestedSelect = createMemo<NodeSelect>(
    () => app().status().requested_node_select || app().status().node_select || 'manual',
  )
  const isManual = createMemo(
    () => requestedSelect() === 'manual' && app().status().node_select === 'manual',
  )

  const candidateNodes = createMemo(() => app().primaryGroup()?.all ?? [])
  const currentNode = createMemo(() => app().primaryGroup()?.now ?? '')
  const currentDisplay = createMemo(() => cleanNodeDisplay(currentNode()))

  let prevSelectedNode = ''
  const selectedNodeMap = createProjection<Record<string, boolean>>((draft) => {
    const next = currentNode()
    if (prevSelectedNode && prevSelectedNode !== next) delete draft[prevSelectedNode]
    if (next) draft[next] = true
    prevSelectedNode = next
  }, {})

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

  const openAddSubscription = () => {
    app().setNodesTab('sources')
    setAddingSubOpen(true)
  }

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
      <header class="page-header">
        <div>
          <h1 class="page-title">代理节点</h1>
          <p class="page-subtitle">测速切换默认代理出口，或集中管理订阅链接与自建节点</p>
        </div>
        <div class="page-actions">
          <Button
            tone="secondary"
            size="sm"
            icon={<PlusIcon size={ICON.xs} />}
            onClick={openAddSubscription}
          >
            添加订阅
          </Button>
          <Button
            tone="primary"
            size="sm"
            icon={<ListPlusIcon size={ICON.xs} />}
            onClick={() => app().openNodeModal('link')}
          >
            添加节点
          </Button>
        </div>
      </header>

      <div class="studio-toolbar">
        <div class="seg-rail" role="group" aria-label="代理节点视图切换">
          <button
            type="button"
            class={['seg-btn', { active: tab() === 'pool' }]}
            aria-pressed={tab() === 'pool' ? 'true' : 'false'}
            onClick={() => app().setNodesTab('pool')}
          >
            <span>出口选路</span>
            <Show when={candidateNodes().length > 0}>
              <span class="num">({candidateNodes().length})</span>
            </Show>
          </button>

          <button
            type="button"
            class={['seg-btn', { active: tab() === 'sources' }]}
            aria-pressed={tab() === 'sources' ? 'true' : 'false'}
            onClick={() => app().setNodesTab('sources')}
          >
            <span>节点源管理</span>
            <span class="num">({sourceTotalCount()})</span>
            <Show when={sourceAlertCount() > 0}>
              <span class="status-dot crimson" aria-hidden="true" />
            </Show>
          </button>
        </div>

        <div class="studio-toolbar-right">
          <Show when={tab() === 'pool'}>
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
          </Show>

          <Show when={tab() === 'sources'}>
            <Button
              tone="secondary"
              size="sm"
              icon={<ClockIcon size={ICON.xs} />}
              onClick={props.onOpenSchedule}
            >
              定时刷新
            </Button>
            <Button
              tone="secondary"
              size="sm"
              icon={<RefreshCwIcon size={ICON.xs} />}
              loading={app().pendingActions().has('refreshSubs')}
              disabled={app().subs().length === 0}
              onClick={app().handleOpenRefreshSubscriptionsConfirm}
            >
              刷新全部
            </Button>
          </Show>
        </div>
      </div>

      {/* Tab 1: Default Proxy Exit & Node Matrix */}
      <Show when={tab() === 'pool'}>
        <section class="surface-box" aria-label="默认代理出口">
          <Show when={app().status().ready && candidateNodes().length > 0}>
            <div class="surface-header">
              <div class="surface-title-group">
                <h2 class="surface-title">默认代理出口</h2>
                <Show when={currentNode()}>
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

                <Show when={!isManual()}>
                  <span class="mono-tag cobalt">自动优选</span>
                  <button
                    type="button"
                    class="btn-link"
                    disabled={app().pendingActions().has('nodeSelect')}
                    onClick={() => { void app().handleSetNodeSelect('manual') }}
                  >
                    切回手动
                  </button>
                </Show>
              </div>

              <Show when={candidateNodes().length > 4}>
                <div class="surface-actions">
                  <div class="seg-rail" role="group" aria-label="地区筛选">
                    <For each={regionBuckets()} keyed={(bucket) => bucket.id}>
                      {(bucket) => (
                        <button
                          type="button"
                          class={['seg-btn', { active: region() === bucket().id }]}
                          aria-pressed={region() === bucket().id ? 'true' : 'false'}
                          onClick={() => setRegion(bucket().id)}
                        >
                          {REGION_LABELS[bucket().id]} <span class="num">{bucket().count}</span>
                        </button>
                      )}
                    </For>
                  </div>

                  <Show when={candidateNodes().length > 8}>
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
            </div>
          </Show>

          <div class="surface-body">
            <Show
              when={app().status().ready}
              fallback={
                <div class="empty-state empty-state-stack">
                  <h2 class="sr-only">默认代理出口</h2>
                  <span>代理服务当前未运行，启动服务后即可查看实时节点池并测速切换。</span>
                  <div class="empty-state-actions">
                    <Button
                      tone="primary"
                      size="sm"
                      icon={<PlayIcon size={ICON.xs} />}
                      loading={app().pendingActions().has('startService')}
                      onClick={() => { void app().handleStartService() }}
                    >
                      启动服务
                    </Button>
                    <Button
                      tone="secondary"
                      size="sm"
                      onClick={() => app().setNodesTab('sources')}
                    >
                      管理节点源 ({sourceTotalCount()})
                    </Button>
                  </div>
                </div>
              }
            >
              <Show
                when={candidateNodes().length > 0}
                fallback={
                  <div class="empty-state empty-state-stack">
                    <h2 class="sr-only">默认代理出口</h2>
                    <span>当前没有可用的候选节点，请先接入订阅链接或添加自建节点。</span>
                    <div class="empty-state-actions">
                      <Button tone="secondary" size="sm" onClick={openAddSubscription}>
                        添加订阅
                      </Button>
                      <Button tone="primary" size="sm" onClick={() => app().openNodeModal('link')}>
                        添加节点
                      </Button>
                    </div>
                  </div>
                }
              >
                <Show
                  when={visibleNodes().length > 0}
                  fallback={<div class="empty-state">没有匹配当前筛选条件的节点。</div>}
                >
                  <div class="node-matrix" role="group" aria-label="候选节点列表">
                    <For each={visibleNodes()}>
                      {(nodeName) => {
                        const selected = () => Boolean(selectedNodeMap[nodeName])
                        const cleaned = createMemo(() => cleanNodeDisplay(nodeName))
                        const delay = () => app().delays()[nodeName]
                        const tone = () => getDelayTone(delay())
                        return (
                          <button
                            type="button"
                            class={['node-cell', { selected: selected() }]}
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
                                class={[
                                  'status-dot',
                                  selected()
                                    ? 'emerald'
                                    : tone() === 'timeout'
                                      ? 'crimson'
                                      : 'muted',
                                ]}
                              />
                              <Show when={cleaned().code}>
                                {(code) => <span class="mono-tag">{code()}</span>}
                              </Show>
                              <span class="node-cell-name">{cleaned().name}</span>
                            </span>
                            <span
                              class={[
                                'latency-text',
                                'num',
                                {
                                  fast: tone() === 'fast',
                                  medium: tone() === 'medium',
                                  slow: tone() === 'slow' || tone() === 'timeout',
                                },
                              ]}
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
      </Show>

      {/* Tab 2: Node Sources Management (Subscriptions & Manual Nodes) */}
      <Show when={tab() === 'sources'}>
        <section class="surface-box" aria-label="订阅源">
          <div class="surface-header">
            <h2 class="surface-title">
              <span>订阅源</span>
              <span class="mono-tag num">{app().subs().length}</span>
            </h2>
          </div>

          <Show when={addingSubOpen()}>
            <div class="inline-form-bar">
              <form
                class="inline-form"
                onSubmit={(e) => { void submitSubscription(e) }}
              >
                <input
                  data-autofocus
                  aria-label="订阅地址"
                  placeholder="https://example.com/api/v1/client/subscribe?token=..."
                  value={subUrl()}
                  disabled={app().pendingActions().has('addSub')}
                  onInput={(e) => setSubUrl(e.currentTarget.value)}
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

              <Show when={vergeItems()}>
                {(items) => (
                  <div class="circuit-banner" style={{ 'margin-top': '10px' }}>
                    <span>
                      检测到 {items().length} 个本机 Clash Verge 订阅源（{items().filter((i) => !i.already_added).length} 个未添加）
                    </span>
                    <div class="surface-actions">
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
            </div>
          </Show>

          <Show
            when={app().subs().length > 0}
            fallback={
              <div class="empty-state">
                尚未添加订阅源，点击右上角「添加订阅」接入 Clash YAML 订阅链接。
              </div>
            }
          >
            <div class="hairline-list" role="list" aria-label="订阅源列表">
              <For each={app().subs()} keyed={(sub) => sub.url}>
                {(sub) => {
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
                    <div class="hairline-row" role="listitem">
                      <div class="row-main">
                        <div class="row-title sub-url-title">
                          <span class={['status-dot', dotTone()]} aria-hidden="true" />
                          <span class="mono truncate">{maskSubscription(sub().url)}</span>
                        </div>
                        <div class={['row-sub', { 'text-danger': sub().state === 'failed' }]}>
                          {formatSubStateText(sub())}
                        </div>
                      </div>
                      <div class="row-end">
                        <Button
                          tone="secondary"
                          size="sm"
                          onClick={() => props.onInspectSub(sub())}
                        >
                          管理节点
                        </Button>
                        <button
                          type="button"
                          class="icon-button danger"
                          aria-label={`删除订阅 ${maskSubscription(sub().url)}`}
                          title="删除订阅源"
                          onClick={() => app().handleOpenDeleteSubConfirm(sub().url)}
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

        <section class="surface-box" aria-label="手动节点">
          <div class="surface-header">
            <h2 class="surface-title">
              <span>手动节点</span>
              <span class="mono-tag num">{manualLines().length}</span>
            </h2>
            <Show when={app().status().vps_supported !== false}>
              <div class="surface-actions">
                <Button
                  tone="ghost"
                  size="sm"
                  icon={<RocketIcon size={ICON.xs} />}
                  onClick={() => app().openNodeModal('vps')}
                >
                  SSH 部署 VPS
                </Button>
              </div>
            </Show>
          </div>

          <Show
            when={manualLines().length > 0}
            fallback={
              <div class="empty-state">
                暂无自建或手动导入的节点。点击右上角「添加节点」可粘贴分享链接、手动配置参数或部署 VPS。
              </div>
            }
          >
            <div class="hairline-list">
              <For each={manualLines()} keyed={(row) => row.node.tag}>
                {(row) => {
                  const cleaned = () => cleanNodeDisplay(row().node.tag)
                  const tone = () => getDelayTone(row().delay)
                  const bindingsSummary = () => {
                    const parts: string[] = []
                    if (row().boundSeatCount > 0) parts.push(`已绑定 ${row().boundSeatCount} 台设备`)
                    if (row().boundPortCount > 0) parts.push(`已绑定 ${row().boundPortCount} 个端口`)
                    return parts.join(' · ')
                  }

                  return (
                    <div class={['hairline-row', { 'danger-tint': !row().relayExists }]}>
                      <div class="row-main">
                        <div class="row-title">
                          <span
                            class={[
                              'status-dot',
                              !row().relayExists
                                ? 'crimson'
                                : tone() === 'timeout'
                                  ? 'amber'
                                  : 'emerald',
                            ]}
                          />
                          <Show when={cleaned().code}>
                            {(code) => <span class="mono-tag">{code()}</span>}
                          </Show>
                          <span>{cleaned().name}</span>
                          <span class="mono-tag">{row().protocolLabel}</span>
                        </div>
                        <div class="row-sub mono">
                          {row().node.server}:{row().node.server_port}
                          {row().node.sni ? ` · SNI ${row().node.sni}` : ''}
                          {row().detourRelay
                            ? ` · 前置跳板: ${cleanNodeDisplay(row().detourRelay!).name}${!row().relayExists ? ' (缺失)' : ''}`
                            : ''}
                          {bindingsSummary() ? ` · ${bindingsSummary()}` : ''}
                        </div>
                      </div>

                      <div class="row-end">
                        <span
                          class={[
                            'latency-text',
                            'num',
                            {
                              fast: tone() === 'fast',
                              medium: tone() === 'medium',
                              slow: tone() === 'slow' || tone() === 'timeout',
                            },
                          ]}
                        >
                          {row().testing
                            ? '测速中'
                            : row().delay == null
                              ? '—'
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
                          aria-label={`删除节点 ${row().node.tag}`}
                          title="删除节点"
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
      </Show>
    </>
  )
}
