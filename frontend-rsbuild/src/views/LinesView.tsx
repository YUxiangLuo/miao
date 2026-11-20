import { For, Show, createMemo, createSignal } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { buildProxyRegionBuckets, filterAndPinProxyNodes, type ProxyRegionId } from '../proxyRegions'
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
  GlobeIcon,
  LockIcon,
  PlusIcon,
  RefreshCwIcon,
  RocketIcon,
  RssIcon,
  SearchIcon,
  ServerIcon,
  Trash2Icon,
  ZapIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

const NODE_SELECT_OPTIONS: ReadonlyArray<{ value: NodeSelect; label: string }> = [
  { value: 'manual', label: '手动固定' },
  { value: 'fastest_hk', label: '香港 自动最快' },
  { value: 'fastest_jp', label: '日本 自动最快' },
  { value: 'fastest_sg', label: '新加坡 自动最快' },
  { value: 'fastest_tw', label: '台湾 自动最快' },
  { value: 'fastest_us', label: '美国 自动最快' },
]

export function LinesView(props: { app: AppController; onInspectSub: (sub: SubStatus) => void; onOpenSchedule: () => void }) {
  const app = () => props.app

  const [subUrl, setSubUrl] = createSignal('')
  const [addingSubForm, setAddingSubForm] = createSignal(false)
  const [vergeItems, setVergeItems] = createSignal<VergeImportItem[] | null>(null)
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
  const visiblePoolNodes = createMemo(() => filterAndPinProxyNodes(
    candidateNodes(),
    currentSelectedNode(),
    poolRegion(),
    poolQuery(),
  ))

  const submitSubscription = async () => {
    if (!subUrl().trim()) return
    const ok = await app().handleAddSubscription(subUrl())
    if (ok) {
      setSubUrl('')
      setAddingSubForm(false)
    }
  }

  const scanVerge = async () => {
    const result = await app().scanClashVerge()
    if (!result || !result.found || result.items.length === 0) {
      app().showToast('未在本机检测到可导入的 Clash Verge 线路源', 'info')
      return
    }
    setVergeItems(result.items)
  }

  const importAvailableVerge = async () => {
    const urls = (vergeItems() || []).filter((item) => !item.already_added).map((item) => item.url)
    if (urls.length === 0) {
      setVergeItems(null)
      return
    }
    if (await app().importClashVergeSubs(urls)) {
      setVergeItems(null)
    }
  }

  return (
    <>
      {/* Upper Section: Dedicated Static Lines */}
      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <LockIcon size={ICON.md} class="icon-accent" />
              独享静态专线资产 ({dedicatedLines().length})
            </h2>
            <p>
              用于绑定固定员工工位或多店铺专属端口的干净独享 IP（支持海外静态住宅 SOCKS5 + 香港/新加坡前置跳板链式转发，以及一键部署自建 Hysteria2 专线 VPS）。
            </p>
          </div>
          <div class="hg-card-actions">
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={() => app().openNodeModal('socks-residential')}
            >
              录入静态住宅 SOCKS5
            </Button>
            <Button
              tone="secondary"
              size="sm"
              icon={<RocketIcon size={ICON.xs} />}
              onClick={() => app().openNodeModal('link')}
            >
              VPS 部署 / 链接导入
            </Button>
          </div>
        </div>

        <Show
          when={dedicatedLines().length > 0}
          fallback={
            <div class="empty-block">
              尚未录入独享静态专线。外贸多店铺防关联建议点击右上角「录入静态住宅 SOCKS5」或部署一台独享海外云服务器。
            </div>
          }
        >
          <div class="hg-table-wrap">
            <table class="hg-table">
              <thead>
                <tr>
                  <th>专线名称 / 类型</th>
                  <th>落地服务器地址</th>
                  <th>出境转发拓扑链路</th>
                  <th>关联工位 / 店铺端口</th>
                  <th>连通延迟</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                <For each={dedicatedLines()} keyed={(row) => row.node.tag}>
                  {(row) => {
                    const tone = () => getDelayTone(row().delay)
                    return (
                      <tr class={classNames(!row().relayExists && 'row-danger')}>
                        <td>
                          <div class="hg-entity-primary">
                            <span>{row().node.tag}</span>
                            <span class={classNames('badge', row().isStaticResidential ? 'emerald' : 'ocean')}>
                              {row().protocolLabel}
                            </span>
                          </div>
                        </td>
                        <td class="mono">
                          <div>{row().node.server}:{row().node.server_port}</div>
                          <Show when={row().node.sni}>
                            {(sni) => <div class="hg-entity-sub">SNI: {sni()}</div>}
                          </Show>
                        </td>
                        <td>
                          <div class="topo-chain">
                            <span class="topo-node">网关</span>
                            <Show when={row().detourRelay}>
                              {(relay) => (
                                <>
                                  <span class="topo-arrow">→</span>
                                  <span class={classNames('topo-node', row().relayExists ? 'relay' : '')}>
                                    {row().relayExists ? `前置跳板 ${relay()}` : `⛔ 缺失跳板 ${relay()}`}
                                  </span>
                                </>
                              )}
                            </Show>
                            <span class="topo-arrow">→</span>
                            <span class="topo-node exit">{row().node.tag}</span>
                          </div>
                        </td>
                        <td class="num">
                          <span>{row().boundSeatCount} 工位</span>
                          <span style={{ margin: '0 6px', color: 'var(--text-muted)' }}>·</span>
                          <span>{row().boundPortCount} 店铺端口</span>
                        </td>
                        <td class="num">
                          <span
                            class={classNames(
                              'badge',
                              tone() === 'fast'
                                ? 'emerald'
                                : tone() === 'medium'
                                  ? 'ocean'
                                  : tone() === 'timeout' || tone() === 'slow'
                                    ? 'danger'
                                    : 'slate',
                            )}
                          >
                            {row().testing ? '测速中…' : formatDelay(row().delay)}
                          </span>
                        </td>
                        <td>
                          <div class="hg-inline-actions">
                            <Button
                              tone="ghost"
                              size="sm"
                              disabled={!app().status().ready || row().testing}
                              onClick={() => app().handleTestDelay(row().node.tag)}
                            >
                              测速
                            </Button>
                            <button
                              type="button"
                              class="icon-button danger"
                              aria-label={`删除专线 ${row().node.tag}`}
                              title="删除此独享专线"
                              onClick={() => app().handleOpenDeleteNodeConfirm(row().node.tag)}
                            >
                              <Trash2Icon size={ICON.sm} />
                            </button>
                          </div>
                        </td>
                      </tr>
                    )
                  }}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </section>

      {/* Lower Section: Shared Outbound Pool & Subscriptions */}
      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <GlobeIcon size={ICON.md} class="icon-accent" />
              公共出海线路池与订阅源 ({app().subs().length} 源 · {candidateNodes().length} 可选节点)
            </h2>
            <p>
              供普通办公检索、Google / AI 工具加载以及作为海外静态住宅 SOCKS5 的高带宽前置中转跳板池。
            </p>
          </div>
          <div class="hg-card-actions">
            <Button
              tone="secondary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={() => setAddingSubForm((v) => !v)}
            >
              添加线路订阅源
            </Button>
            <Button
              tone="secondary"
              size="sm"
              icon={<DownloadIcon size={ICON.xs} />}
              onClick={() => { void scanVerge() }}
            >
              导入 Clash Verge
            </Button>
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
              立即更新全部源
            </Button>
          </div>
        </div>

        <div class="hg-card-body" style={{ display: 'flex', 'flex-direction': 'column', gap: '16px' }}>
          <Show when={addingSubForm()}>
            <div class="list-row" style={{ gap: '10px', 'flex-wrap': 'wrap' }}>
              <input
                style={{ flex: '1', 'min-width': '260px', height: '36px', padding: '0 12px', 'border-radius': '6px', border: '1px solid var(--border-strong)', background: 'var(--surface)' }}
                placeholder="粘贴企业机场或专线池 HTTPS 订阅地址…"
                value={subUrl()}
                onInput={(e) => setSubUrl(e.currentTarget.value)}
                onKeyDown={(e) => { if (e.key === 'Enter') void submitSubscription() }}
              />
              <Button
                tone="primary"
                size="sm"
                loading={app().pendingActions().has('addSub')}
                onClick={() => { void submitSubscription() }}
              >
                保存并拉取节点
              </Button>
              <Button tone="ghost" size="sm" onClick={() => setAddingSubForm(false)}>取消</Button>
            </div>
          </Show>

          <Show when={vergeItems()}>
            {(items) => (
              <div class="hg-alert info">
                <div class="hg-alert-main">
                  <RssIcon size={ICON.lg} />
                  <div>
                    <strong>检测到 {items().length} 个本地 Clash Verge 订阅源</strong>
                    <p>其中 {items().filter((i) => !i.already_added).length} 个尚未加入当前网关。</p>
                  </div>
                </div>
                <div class="hg-inline-actions">
                  <Button tone="primary" size="sm" onClick={() => { void importAvailableVerge() }}>
                    一键导入未添加源
                  </Button>
                  <Button tone="ghost" size="sm" onClick={() => setVergeItems(null)}>关闭</Button>
                </div>
              </div>
            )}
          </Show>

          {/* Subscription Table */}
          <Show when={app().subs().length > 0}>
            <div class="hg-table-wrap">
              <table class="hg-table">
                <thead>
                  <tr>
                    <th>订阅源地址</th>
                    <th>同步状态</th>
                    <th>可用节点数</th>
                    <th>操作</th>
                  </tr>
                </thead>
                <tbody>
                  <For each={app().subs()} keyed={(sub) => sub.url}>
                    {(sub) => (
                      <tr>
                        <td class="mono" title={sub().url}>{maskSubscription(sub().url)}</td>
                        <td>
                          <span
                            class={classNames(
                              'badge',
                              sub().state === 'ready'
                                ? 'emerald'
                                : sub().state === 'refreshing'
                                  ? 'ocean'
                                  : sub().state === 'failed'
                                    ? 'danger'
                                    : 'slate',
                            )}
                          >
                            {sub().state === 'ready'
                              ? '同步正常'
                              : sub().state === 'refreshing'
                                ? '正在拉取…'
                                : sub().state === 'failed'
                                  ? `失败${sub().error ? `: ${sub().error}` : ''}`
                                  : '等待拉取'}
                          </span>
                        </td>
                        <td class="num">
                          {sub().node_count} 节点
                          {sub().disabled_count > 0 ? ` (已屏蔽 ${sub().disabled_count})` : ''}
                        </td>
                        <td>
                          <div class="hg-inline-actions">
                            <Button tone="ghost" size="sm" onClick={() => props.onInspectSub(sub())}>
                              查看 / 屏蔽单节点
                            </Button>
                            <button
                              type="button"
                              class="icon-button danger"
                              aria-label={`删除订阅 ${maskSubscription(sub().url)}`}
                              onClick={() => app().handleOpenDeleteSubConfirm(sub().url)}
                            >
                              <Trash2Icon size={ICON.sm} />
                            </button>
                          </div>
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Show>

          {/* Pool Default Selection Policy */}
          <div class="hg-toolbar">
            <div class="hg-toolbar-group">
              <span style={{ 'font-size': '12.5px', 'font-weight': '600', color: 'var(--text-secondary)' }}>
                公共池默认选路策略：
              </span>
              <div class="hg-segmented" role="group" aria-label="公共池选路策略">
                <For each={NODE_SELECT_OPTIONS}>
                  {(opt) => (
                    <button
                      type="button"
                      class={classNames('hg-segment', requestedSelect() === opt.value && 'active')}
                      aria-pressed={requestedSelect() === opt.value ? 'true' : 'false'}
                      disabled={app().pendingActions().has('nodeSelect')}
                      onClick={() => { void app().handleSetNodeSelect(opt.value) }}
                    >
                      {opt.label}
                    </button>
                  )}
                </For>
              </div>
            </div>

            <div class="hg-toolbar-group">
              <Show when={requestedSelect() !== 'manual' && app().status().multiplier_options.length > 0}>
                <select
                  class="hg-select"
                  aria-label="最高节点倍率限制"
                  value={app().status().max_multiplier ?? ''}
                  onChange={(e) => {
                    const v = e.currentTarget.value
                    void app().handleSetMaxMultiplier(v === '' ? null : v)
                  }}
                >
                  <option value="">不限倍率</option>
                  <For each={app().status().multiplier_options}>
                    {(mult) => <option value={mult}>最高 {mult}x 倍率</option>}
                  </For>
                </select>
              </Show>

              <Show when={app().status().ready && candidateNodes().length > 0}>
                <Button
                  tone="secondary"
                  size="sm"
                  icon={<ZapIcon size={ICON.xs} />}
                  loading={Boolean(app().testingGroup())}
                  onClick={() => app().handleTestGroupDelays(app().primaryGroupName() || 'proxy', candidateNodes())}
                >
                  批量测速 ({candidateNodes().length})
                </Button>
              </Show>
            </div>
          </div>

          <Show when={candidateNodes().length > 0}>
            <div class="hg-toolbar">
              <div class="hg-segmented" role="group" aria-label="线路池地区过滤">
                <For each={regionBuckets()}>
                  {(bucket) => (
                    <button
                      type="button"
                      class={classNames('hg-segment', poolRegion() === bucket.id && 'active')}
                      aria-pressed={poolRegion() === bucket.id ? 'true' : 'false'}
                      onClick={() => setPoolRegion(bucket.id)}
                    >
                      {bucket.label} ({bucket.count})
                    </button>
                  )}
                </For>
              </div>
              <div class="hg-search">
                <SearchIcon size={ICON.sm} />
                <input
                  type="search"
                  aria-label="搜索公共池节点"
                  placeholder="搜索节点地区或编号…"
                  value={poolQuery()}
                  onInput={(e) => setPoolQuery(e.currentTarget.value)}
                />
              </div>
            </div>

            <div class="hg-pool-nodes-grid">
              <For each={visiblePoolNodes()}>
                {(nodeName) => {
                  const selected = () => currentSelectedNode() === nodeName
                  const delay = () => app().delays()[nodeName]
                  const tone = () => getDelayTone(delay())
                  return (
                    <button
                      type="button"
                      class={classNames('hg-pool-node-btn', selected() && 'selected')}
                      aria-pressed={selected() ? 'true' : 'false'}
                      disabled={requestedSelect() !== 'manual' || Boolean(app().switchingNode())}
                      title={requestedSelect() !== 'manual' ? '当前为自动优选模式；切换到「手动固定」后可手动点选公共池默认出口' : `切换默认公共池出口为 ${nodeName}`}
                      onClick={() => { void app().handleSwitchProxy(app().primaryGroupName() || 'proxy', nodeName) }}
                    >
                      <div style={{ 'min-width': '0', flex: '1' }}>
                        <div class="hg-pool-node-name">{nodeName}</div>
                        <div class="hg-entity-sub">
                          <ServerIcon size={11} /> {app().nodeProtocols()[nodeName] || '代理出口'}
                        </div>
                      </div>
                      <span
                        class={classNames(
                          'badge',
                          'num',
                          tone() === 'fast'
                            ? 'emerald'
                            : tone() === 'medium'
                              ? 'ocean'
                              : tone() === 'timeout' || tone() === 'slow'
                                ? 'danger'
                                : 'slate',
                        )}
                      >
                        {app().testingNodes()[nodeName] ? '…' : formatDelay(delay())}
                      </span>
                    </button>
                  )
                }}
              </For>
            </div>
          </Show>
        </div>
      </section>
    </>
  )
}
