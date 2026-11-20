import { For, Show, createMemo, createSignal, createUniqueId } from 'solid-js'
import { Portal } from '@solidjs/web'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
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
import { buildTerminalRows, isSingleSeatRule } from '../domain/terminals'
import { buildAccessPortRows, connectionAccessPortId } from '../domain/accessPorts'
import {
  findPlatformPolicyRule,
  TRADE_PLATFORMS,
  type TradeCategoryId,
  type TradePlatform,
} from '../domain/tradeCatalog'
import { PROTOCOL_OPTIONS, RULE_FIELD_OPTIONS, ruleFieldLabel } from '../ruleFormat'
import { useDialog } from '../hooks/useDialog'
import { classNames, formatDelay, formatSpeed } from '../utils'
import type { RuleInfo } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import { PlusIcon, RouteIcon, SearchIcon, Trash2Icon, XIcon } from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export type PulseTab = 'stream' | 'rules'

export interface PulseViewProps {
  app: AppController
  initialTab?: PulseTab
}

const STUDIO_CATEGORY_TABS: ReadonlyArray<{ id: TradeCategoryId; label: string }> = [
  { id: 'social', label: '社媒沟通' },
  { id: 'ecommerce', label: '跨境电商' },
  { id: 'payment', label: '收款结算' },
  { id: 'productivity', label: '邮件与 AI' },
  { id: 'domestic', label: '国内直连' },
]

const PRIORITY_STAGES: ReadonlyArray<{ index: string; label: string }> = [
  { index: '01', label: '店铺专属端口' },
  { index: '02', label: '大陆办公直连' },
  { index: '03', label: '设备整机锁定' },
  { index: '04', label: '业务应用规则' },
  { index: '05', label: '公共出海池' },
]

function describeRuleTarget(rule: RuleInfo): {
  tone: 'emerald' | 'cobalt' | 'crimson' | 'muted'
  text: string
} {
  if (rule.skipped) {
    const rawTarget = rule.target ? cleanNodeDisplay(rule.target).name : '未知出口'
    return { tone: 'crimson', text: `${rawTarget} · 离线熔断` }
  }
  if (!rule.target) {
    return { tone: 'muted', text: '自定义动作' }
  }
  if (rule.target === 'direct') {
    return { tone: 'emerald', text: '强制直连' }
  }
  if (rule.target === 'reject') {
    return { tone: 'crimson', text: '阻断访问' }
  }
  if (rule.target === 'proxy') {
    return { tone: 'cobalt', text: '公共出海池' }
  }
  return { tone: 'emerald', text: cleanNodeDisplay(rule.target).name }
}

export function PulseView(props: PulseViewProps) {
  const app = () => props.app

  const [tab, setTab] = createSignal<PulseTab>(props.initialTab ?? 'stream')

  // Stream filter signals
  const [pathFilter, setPathFilter] = createSignal<'all' | 'proxy' | 'direct'>('all')
  const [seatFilter, setSeatFilter] = createSignal('')
  const [portFilter, setPortFilter] = createSignal('')
  const [keyword, setKeyword] = createSignal('')

  // Rules signals
  const [activeCategory, setActiveCategory] = createSignal<TradeCategoryId>('social')
  const [showSeatRules, setShowSeatRules] = createSignal(false)
  const [drawerOpen, setDrawerOpen] = createSignal(false)
  const [field, setField] = createSignal('domain_suffix')
  const [value, setValue] = createSignal('')
  const [target, setTarget] = createSignal('proxy')
  const [saving, setSaving] = createSignal(false)

  const dialogTitleId = createUniqueId()
  const dialogRef = useDialog<HTMLFormElement>(drawerOpen, () => setDrawerOpen(false))

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
    { value: '', label: '全部设备' },
    ...terminalRows().map((row) => ({
      value: row.ip,
      label: `${row.displayName} (${row.ip})`,
      badge: row.connectionCount > 0 ? `${row.connectionCount}` : undefined,
      tone: row.failClosed
        ? ('crimson' as const)
        : row.online
          ? ('emerald' as const)
          : ('default' as const),
    })),
  ])

  const portOptions = createMemo<SelectOption[]>(() => [
    { value: '', label: '全部店铺端口' },
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

  // Rules memos & options
  const categoryPlatforms = createMemo(() => (
    TRADE_PLATFORMS.filter((item) => item.category === activeCategory())
  ))

  const visibleRules = createMemo(() => (
    showSeatRules()
      ? app().rules()
      : app().rules().filter((rule) => !isSingleSeatRule(rule))
  ))

  const dedicatedNodes = createMemo(() => app().nodes())
  const dedicatedTagSet = createMemo(() => new Set(dedicatedNodes().map((n) => n.tag)))
  const poolNodeTags = createMemo(() => (
    app().ruleNodeNames().filter((tag) => !dedicatedTagSet().has(tag))
  ))

  const buildNodeGroupOptions = (): SelectOption[] => {
    const delays = app().delays()
    const dedicatedOpts: SelectOption[] = dedicatedNodes().map((node) => ({
      value: node.tag,
      label: node.tag,
      group: '独享静态 IP',
      badge: delays[node.tag] != null ? formatDelay(delays[node.tag]) : node.node_type.toUpperCase(),
      tone: 'emerald',
    }))
    const poolOpts: SelectOption[] = poolNodeTags().map((tag) => ({
      value: tag,
      label: tag,
      group: '公共池节点',
      badge: delays[tag] != null ? formatDelay(delays[tag]) : undefined,
      tone: 'cobalt',
    }))
    return [...dedicatedOpts, ...poolOpts]
  }

  const platformTargetOptions = (platform: TradePlatform, currentTarget?: string): SelectOption[] => {
    const base: SelectOption[] = [
      {
        value: '',
        label: platform.category === 'domestic' ? '默认（大陆规则直连）' : '默认（跟随设备/公共池）',
        tone: 'default',
      },
      { value: 'proxy', label: '公共出海池', tone: 'cobalt' },
      { value: 'direct', label: '强制直连', tone: 'emerald' },
      { value: 'reject', label: '阻断访问', tone: 'crimson' },
      ...buildNodeGroupOptions(),
    ]
    if (currentTarget && !base.some((opt) => opt.value === currentTarget)) {
      base.push({
        value: currentTarget,
        label: `${cleanNodeDisplay(currentTarget).name}（离线）`,
        group: '当前绑定',
        tone: 'crimson',
      })
    }
    return base
  }

  const customRuleFieldOptions: SelectOption[] = RULE_FIELD_OPTIONS.map((opt) => ({
    value: opt.value,
    label: opt.label,
  }))

  const protocolSelectOptions: SelectOption[] = PROTOCOL_OPTIONS.map((opt) => ({
    value: opt.value,
    label: opt.label.toUpperCase(),
  }))

  const customRuleTargetOptions = createMemo<SelectOption[]>(() => [
    { value: 'proxy', label: '公共出海池', tone: 'cobalt' },
    { value: 'direct', label: '强制直连', tone: 'emerald' },
    { value: 'reject', label: '阻断访问', tone: 'crimson' },
    ...buildNodeGroupOptions(),
  ])

  const fieldPlaceholder = createMemo(
    () => RULE_FIELD_OPTIONS.find((opt) => opt.value === field())?.placeholder || '请输入匹配值',
  )

  const handleFieldChange = (nextField: string) => {
    setField(nextField)
    if (nextField === 'protocol') {
      if (!PROTOCOL_OPTIONS.some((p) => p.value === value())) {
        setValue('quic')
      }
    } else if (PROTOCOL_OPTIONS.some((p) => p.value === value())) {
      setValue('')
    }
  }

  const handleAssignPlatform = async (platform: TradePlatform, nextTarget: string) => {
    const existing = findPlatformPolicyRule(platform, app().rules())
    if (!nextTarget) {
      if (existing) app().handleOpenDeleteRuleConfirm(existing)
      return
    }
    if (existing && existing.target === nextTarget) return
    if (existing) {
      await app().apiCall(
        'rules',
        { method: 'DELETE', body: JSON.stringify({ index: existing.index, raw: existing.raw }) },
        'deleteRule',
      )
    }
    await app().handleAddRule({
      field: 'domain_suffix',
      value: platform.primarySuffix,
      target: nextTarget,
    })
  }

  const submitCustomRule = async (event?: Event) => {
    event?.preventDefault()
    if (saving() || !value().trim()) return
    setSaving(true)
    try {
      const ok = await app().handleAddRule({
        field: field(),
        value: value().trim(),
        target: target(),
      })
      if (ok) {
        setValue('')
        setDrawerOpen(false)
      }
    } finally {
      setSaving(false)
    }
  }

  return (
    <>
      {/* Top Mode Switcher Bar */}
      <div class="studio-toolbar">
        <div class="seg-rail" role="group" aria-label="分流与脉冲模式">
          <button
            type="button"
            class={classNames('seg-btn', tab() === 'stream' && 'active')}
            aria-pressed={tab() === 'stream' ? 'true' : 'false'}
            onClick={() => setTab('stream')}
          >
            实时脉冲 <span class="num">({connections().length})</span>
          </button>
          <button
            type="button"
            class={classNames('seg-btn', tab() === 'rules' && 'active')}
            aria-pressed={tab() === 'rules' ? 'true' : 'false'}
            onClick={() => setTab('rules')}
          >
            分流规则 <span class="num">({app().rules().length})</span>
          </button>
        </div>

        <Show when={tab() === 'stream'}>
          <div style={{ display: 'flex', 'align-items': 'center', gap: '12px', 'flex-wrap': 'wrap' }}>
            <span class="status-inline num">
              <span class="status-dot cobalt" />
              <span>海外 ↓ {formatSpeed(splitStats().proxy.downloadSpeed)}</span>
              <span style={{ color: 'var(--ink-muted)' }}>·</span>
              <span class="status-dot emerald" />
              <span>直连 ↓ {formatSpeed(splitStats().direct.downloadSpeed)}</span>
            </span>
            <ConnectivityCheck
              status={app().status()}
              backendUnreachable={app().backendUnreachable()}
            />
          </div>
        </Show>
      </div>

      {/* Mode A: Live Connection Stream */}
      <Show when={tab() === 'stream'}>
        <div class="studio-toolbar">
          <div style={{ display: 'flex', 'align-items': 'center', gap: '8px', 'flex-wrap': 'wrap' }}>
            <div class="seg-rail" role="group" aria-label="出口通道筛选">
              <button
                type="button"
                class={classNames('seg-btn', pathFilter() === 'all' && 'active')}
                aria-pressed={pathFilter() === 'all' ? 'true' : 'false'}
                onClick={() => setPathFilter('all')}
              >
                全部 <span class="num">({counts().all})</span>
              </button>
              <button
                type="button"
                class={classNames('seg-btn', pathFilter() === 'proxy' && 'active')}
                aria-pressed={pathFilter() === 'proxy' ? 'true' : 'false'}
                onClick={() => setPathFilter('proxy')}
              >
                海外通道 <span class="num">({counts().proxy})</span>
              </button>
              <button
                type="button"
                class={classNames('seg-btn', pathFilter() === 'direct' && 'active')}
                aria-pressed={pathFilter() === 'direct' ? 'true' : 'false'}
                onClick={() => setPathFilter('direct')}
              >
                国内直连 <span class="num">({counts().direct})</span>
              </button>
            </div>

            <SelectPopover
              ariaLabel="按办公设备筛选"
              size="sm"
              value={seatFilter()}
              options={seatOptions()}
              searchPlaceholder="搜索设备名称或 IP…"
              onChange={setSeatFilter}
            />

            <Show when={accessPortRows().length > 0}>
              <SelectPopover
                ariaLabel="按店铺端口筛选"
                size="sm"
                value={portFilter()}
                options={portOptions()}
                searchPlaceholder="搜索店铺名或端口…"
                onChange={setPortFilter}
              />
            </Show>
          </div>

          <div class="studio-search">
            <SearchIcon size={ICON.sm} />
            <input
              type="search"
              aria-label="搜索实时连接"
              placeholder="搜索域名、IP 或出口…"
              value={keyword()}
              onInput={(e) => setKeyword(e.currentTarget.value)}
            />
          </div>
        </div>

        <section class="surface-box" aria-label="实时连接流">
          <Show
            when={app().status().ready}
            fallback={
              <div class="empty-state empty-block">
                网关引擎当前未运行，启动后可实时查看设备与店铺连接脉冲。
              </div>
            }
          >
            <Show
              when={filteredConnections().length > 0}
              fallback={
                <div class="empty-state empty-block">
                  当前筛选条件下暂无活跃连接。
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
      </Show>

      {/* Mode B: Routing Rules */}
      <Show when={tab() === 'rules'}>
        <div class="priority-strip" aria-label="路由匹配优先级">
          <For each={PRIORITY_STAGES}>
            {(stage, idx) => (
              <>
                <Show when={idx() > 0}>
                  <span class="priority-sep" aria-hidden="true">→</span>
                </Show>
                <span class="priority-step">
                  <span class="mono-tag num">{stage.index}</span>
                  <span>{stage.label}</span>
                </span>
              </>
            )}
          </For>
        </div>

        {/* Foreign Trade Business Quick Routing */}
        <section class="surface-box" aria-label="常用业务定向">
          <div class="surface-header">
            <h2 class="surface-title">常用业务定向</h2>
            <div class="seg-rail" role="group" aria-label="业务分类">
              <For each={STUDIO_CATEGORY_TABS}>
                {(cat) => (
                  <button
                    type="button"
                    class={classNames('seg-btn', activeCategory() === cat.id && 'active')}
                    aria-pressed={activeCategory() === cat.id ? 'true' : 'false'}
                    onClick={() => setActiveCategory(cat.id)}
                  >
                    {cat.label}
                  </button>
                )}
              </For>
            </div>
          </div>

          <div class="hairline-list">
            <For each={categoryPlatforms()} keyed={(p) => p.id}>
              {(platform) => {
                const matchedRule = () => findPlatformPolicyRule(platform(), app().rules())
                return (
                  <div class="hairline-row">
                    <div
                      class="row-main"
                      style={{ display: 'flex', 'align-items': 'center', gap: '12px', 'min-width': '0' }}
                    >
                      <span class="app-mark" style={{ background: platform().accent }}>
                        {platform().shortLabel}
                      </span>
                      <div style={{ 'min-width': '0', flex: '1' }}>
                        <div class="row-title">
                          <span>{platform().name}</span>
                          <span class="mono-tag">*.{platform().primarySuffix}</span>
                        </div>
                        <div class="row-sub">{platform().description}</div>
                      </div>
                    </div>

                    <div class="row-end">
                      <div class="endpoint-picker-wrap">
                        <SelectPopover
                          ariaLabel={`${platform().name} 分流出口`}
                          size="sm"
                          align="right"
                          fullWidth
                          value={matchedRule()?.target || ''}
                          options={platformTargetOptions(platform(), matchedRule()?.target)}
                          disabled={app().pendingActions().has('addRule')}
                          searchPlaceholder="搜索专线或节点…"
                          onChange={(val) => {
                            void handleAssignPlatform(platform(), val)
                          }}
                        />
                      </div>
                    </div>
                  </div>
                )
              }}
            </For>
          </div>
        </section>

        {/* Custom Domain / IP / Port Rules */}
        <section class="surface-box" aria-label="自定义规则">
          <div class="surface-header">
            <h2 class="surface-title">
              <span>自定义域名 / IP / 端口规则</span>
              <span class="mono-tag num">{visibleRules().length}</span>
            </h2>
            <div class="surface-actions">
              <label class="field checkbox-field" style={{ 'font-weight': '500' }}>
                <input
                  type="checkbox"
                  checked={showSeatRules()}
                  onChange={(e) => setShowSeatRules(e.currentTarget.checked)}
                />
                <span>显示设备整机规则</span>
              </label>
              <Button
                tone="primary"
                size="sm"
                icon={<PlusIcon size={ICON.xs} />}
                onClick={() => setDrawerOpen(true)}
              >
                新增规则
              </Button>
            </div>
          </div>

          <Show
            when={visibleRules().length > 0}
            fallback={
              <div class="empty-state empty-block">
                暂无自定义分流规则。
              </div>
            }
          >
            <div class="hairline-list">
              <For each={visibleRules()} keyed={(rule) => `${rule.index}:${rule.raw}`}>
                {(rule) => {
                  const targetState = () => describeRuleTarget(rule())
                  return (
                    <div class={classNames('hairline-row', rule().skipped && 'danger-tint')}>
                      <div class="row-main">
                        <div class="row-title">
                          <span class="mono-tag num">#{rule().index + 1}</span>
                          <span class="mono-tag">
                            {rule().field ? ruleFieldLabel(rule().field!) : 'JSON'}
                          </span>
                          <span class="mono" style={{ 'word-break': 'break-all' }}>
                            {rule().value || rule().raw}
                          </span>
                        </div>
                      </div>

                      <div class="row-end">
                        <span class="status-inline">
                          <span class={classNames('status-dot', targetState().tone)} />
                          <span>{targetState().text}</span>
                        </span>
                        <button
                          type="button"
                          class="icon-button danger"
                          aria-label={`删除规则 #${rule().index + 1}`}
                          title="删除规则"
                          onClick={() => app().handleOpenDeleteRuleConfirm(rule())}
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

      {/* Add Custom Rule Dialog */}
      <Show when={drawerOpen()}>
        <Portal>
          <div class="modal-overlay drawer-overlay" onClick={() => setDrawerOpen(false)}>
            <form
              ref={dialogRef}
              class="modal-card drawer-panel"
              role="dialog"
              aria-modal="true"
              aria-labelledby={dialogTitleId}
              tabindex={-1}
              onClick={(e) => e.stopPropagation()}
              onSubmit={(e) => { void submitCustomRule(e) }}
            >
              <div class="modal-title-row">
                <div class="modal-title-wrap">
                  <RouteIcon size={ICON.lg} class="icon-accent" />
                  <h3 id={dialogTitleId}>新增分流规则</h3>
                </div>
                <button
                  type="button"
                  class="icon-button"
                  aria-label="关闭新增规则窗口"
                  onClick={() => setDrawerOpen(false)}
                >
                  <XIcon size={ICON.md} />
                </button>
              </div>

              <div class="form-grid single">
                <div class="field">
                  <span>匹配类型</span>
                  <SelectPopover
                    ariaLabel="匹配类型"
                    fullWidth
                    value={field()}
                    options={customRuleFieldOptions}
                    searchableThreshold={12}
                    onChange={handleFieldChange}
                  />
                </div>
              </div>

              <div class="form-grid single">
                <Show
                  when={field() === 'protocol'}
                  fallback={
                    <label class="field">
                      <span>匹配值</span>
                      <input
                        data-autofocus
                        aria-label="匹配值"
                        placeholder={fieldPlaceholder()}
                        value={value()}
                        onInput={(e) => setValue(e.currentTarget.value)}
                      />
                    </label>
                  }
                >
                  <div class="field">
                    <span>嗅探协议</span>
                    <SelectPopover
                      ariaLabel="嗅探协议"
                      fullWidth
                      value={value() || 'quic'}
                      options={protocolSelectOptions}
                      searchableThreshold={12}
                      onChange={setValue}
                    />
                  </div>
                </Show>
              </div>

              <div class="form-grid single">
                <div class="field">
                  <span>目标出口</span>
                  <SelectPopover
                    ariaLabel="目标出口"
                    fullWidth
                    value={target()}
                    options={customRuleTargetOptions()}
                    searchPlaceholder="搜索专线或节点…"
                    onChange={setTarget}
                  />
                </div>
              </div>

              <div class="modal-actions">
                <Button tone="ghost" size="sm" onClick={() => setDrawerOpen(false)}>
                  取消
                </Button>
                <Button
                  type="submit"
                  tone="primary"
                  size="sm"
                  loading={saving()}
                  disabled={!value().trim()}
                >
                  添加规则
                </Button>
              </div>
            </form>
          </div>
        </Portal>
      </Show>
    </>
  )
}
