import { For, Show, createMemo, createSignal, createUniqueId } from 'solid-js'
import { Portal, type JSX } from '@solidjs/web'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import { useDialog } from '../hooks/useDialog'
import { formatDelay, formatSpeed } from '../utils'
import { buildTerminalRows, isSingleSeatRule, type TerminalRow } from '../domain/terminals'
import { buildAccessPortRows, suggestNextAccessPort, type AccessPortRow } from '../domain/accessPorts'
import { RULE_PRESETS, type RulePreset } from '../domain/rulePresets'
import { PROTOCOL_OPTIONS, ruleFieldLabel, ruleFieldOptions } from '../ruleFormat'
import type { RoutingTabKey } from '../hooks/useAppData'
import type { RuleInfo, TerminalAssignMode } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import {
  CopyIcon,
  EditIcon,
  MonitorSmartphoneIcon,
  NetworkIcon,
  PlusIcon,
  RouteIcon,
  SearchIcon,
  Trash2Icon,
  XIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>
type DeviceFilter = 'all' | 'dedicated' | 'pool' | 'block_overseas' | 'fail_closed'
type RuleScopeFilter = 'general' | 'all'

const DEVICE_FILTER_OPTIONS: SelectOption[] = [
  { value: 'all', label: '全部出口策略' },
  { value: 'dedicated', label: '指定节点' },
  { value: 'pool', label: '默认代理' },
  { value: 'block_overseas', label: '禁止走代理' },
  { value: 'fail_closed', label: '节点不可用（已阻断）' },
]

function encodeSeatSelectValue(mode: TerminalAssignMode, target?: string): string {
  if (mode === 'dedicated' && target) return `dedicated:${target}`
  return mode
}

function deviceDotTone(row: TerminalRow): 'emerald' | 'cobalt' | 'amber' | 'crimson' | 'muted' {
  if (row.failClosed) return 'crimson'
  if (row.mode === 'dedicated') return 'emerald'
  if (row.mode === 'block_overseas') return 'amber'
  if (row.mode === 'direct') return 'muted'
  return 'cobalt'
}

function describeRuleTarget(rule: RuleInfo): {
  tone: 'emerald' | 'cobalt' | 'crimson' | 'muted'
  text: string
} {
  if (rule.skipped) {
    const rawTarget = rule.target ? cleanNodeDisplay(rule.target).name : '未知节点'
    return { tone: 'crimson', text: `${rawTarget} · 节点不可用（已阻断）` }
  }
  if (!rule.target) {
    return { tone: 'muted', text: '自定义动作' }
  }
  if (rule.target === 'direct') {
    return { tone: 'emerald', text: '直连' }
  }
  if (rule.target === 'reject') {
    return { tone: 'crimson', text: '拦截' }
  }
  if (rule.target === 'proxy') {
    return { tone: 'cobalt', text: '默认代理' }
  }
  return { tone: 'emerald', text: cleanNodeDisplay(rule.target).name }
}

export function RoutingView(props: { app: AppController }): JSX.Element {
  const app = () => props.app

  const [deviceFilter, setDeviceFilter] = createSignal<DeviceFilter>('all')
  const [ruleScope, setRuleScope] = createSignal<RuleScopeFilter>('general')
  const [query, setQuery] = createSignal('')

  // Dialog 1: Device profile & optional assignment
  const [deviceDialogOpen, setDeviceDialogOpen] = createSignal(false)
  const [editingIp, setEditingIp] = createSignal('')
  const [ipLocked, setIpLocked] = createSignal(false)
  const [formName, setFormName] = createSignal('')
  const [formDept, setFormDept] = createSignal('')
  const [formNote, setFormNote] = createSignal('')
  const [formAssign, setFormAssign] = createSignal('keep')
  const [initialAssign, setInitialAssign] = createSignal('keep')
  const [savingDevice, setSavingDevice] = createSignal(false)

  const deviceTitleId = createUniqueId()
  const deviceDialogRef = useDialog(deviceDialogOpen, () => setDeviceDialogOpen(false))

  // Dialog 2: Independent proxy port
  const [portDialogOpen, setPortDialogOpen] = createSignal(false)
  const [editPortId, setEditPortId] = createSignal<string | undefined>(undefined)
  const [portName, setPortName] = createSignal('')
  const [portNumber, setPortNumber] = createSignal<number>(20101)
  const [portOutbound, setPortOutbound] = createSignal('')
  const [portUsername, setPortUsername] = createSignal('')
  const [portPassword, setPortPassword] = createSignal('')
  const [savingPort, setSavingPort] = createSignal(false)

  const portTitleId = createUniqueId()
  const portDialogRef = useDialog(portDialogOpen, () => setPortDialogOpen(false))

  // Dialog 3: Custom routing rule
  const [ruleDrawerOpen, setRuleDrawerOpen] = createSignal(false)
  const [ruleField, setRuleField] = createSignal('domain_suffix')
  const [ruleValue, setRuleValue] = createSignal('')
  const [ruleTarget, setRuleTarget] = createSignal('proxy')
  const [savingRule, setSavingRule] = createSignal(false)

  const ruleTitleId = createUniqueId()
  const ruleDialogRef = useDialog<HTMLFormElement>(ruleDrawerOpen, () => setRuleDrawerOpen(false))

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

  const deviceRuleCount = createMemo(() => app().rules().filter(isSingleSeatRule).length)
  const generalRules = createMemo(() => app().rules().filter((rule) => !isSingleSeatRule(rule)))

  const failClosedSeatCount = createMemo(() => terminalRows().filter((row) => row.failClosed).length)
  const failClosedPortCount = createMemo(() => accessPortRows().filter((row) => row.blocked).length)
  const skippedGeneralRuleCount = createMemo(() => generalRules().filter((r) => r.skipped).length)

  const activeTab = createMemo<RoutingTabKey>(() => {
    if (app().routingTabTouched()) return app().routingTab()
    if (failClosedSeatCount() > 0) return 'devices'
    if (failClosedPortCount() > 0) return 'ports'
    if (skippedGeneralRuleCount() > 0) return 'rules'
    if (terminalRows().length === 0 && accessPortRows().length > 0) return 'ports'
    if (terminalRows().length === 0 && accessPortRows().length === 0 && generalRules().length > 0) {
      return 'rules'
    }
    return app().routingTab()
  })

  const switchTab = (next: RoutingTabKey) => {
    setQuery('')
    app().selectRoutingTab(next)
  }

  const manualNodes = createMemo(() => app().nodes())
  const extraPoolNodes = createMemo(() => {
    const manualSet = new Set(manualNodes().map((node) => node.tag))
    return app().ruleNodeNames().filter((tag) => !manualSet.has(tag))
  })

  const filteredDevices = createMemo(() => {
    const q = query().trim().toLowerCase()
    const f = deviceFilter()
    return terminalRows().filter((row) => {
      if (f === 'dedicated' && row.mode !== 'dedicated') return false
      if (f === 'pool' && row.mode !== 'pool') return false
      if (f === 'block_overseas' && row.mode !== 'block_overseas') return false
      if (f === 'fail_closed' && !row.failClosed) return false
      if (!q) return true
      return [
        row.ip,
        row.displayName,
        row.department || '',
        row.note || '',
        row.dhcpHostname || '',
        row.mac || '',
        row.target || '',
      ].some((item) => item.toLowerCase().includes(q))
    })
  })

  const filteredPorts = createMemo(() => {
    const q = query().trim().toLowerCase()
    if (!q) return accessPortRows()
    return accessPortRows().filter((row) => (
      [
        row.name,
        String(row.port),
        row.listenEndpoint,
        row.outbound,
        row.detourRelay || '',
      ].some((item) => item.toLowerCase().includes(q))
    ))
  })

  const visibleRules = createMemo(() => {
    const scope = ruleScope()
    const needle = query().trim().toLowerCase()
    return app().rules().filter((rule) => {
      if (scope === 'general' && isSingleSeatRule(rule)) return false
      if (!needle) return true
      const targetLabel = describeRuleTarget(rule).text.toLowerCase()
      return (
        (rule.value || '').toLowerCase().includes(needle)
        || (rule.field || '').toLowerCase().includes(needle)
        || (rule.target || '').toLowerCase().includes(needle)
        || rule.raw.toLowerCase().includes(needle)
        || targetLabel.includes(needle)
      )
    })
  })

  const ruleScopeOptions = createMemo<SelectOption[]>(() => [
    { value: 'general', label: `常规规则 (${generalRules().length})` },
    { value: 'all', label: `含设备规则 (${app().rules().length})` },
  ])

  const baseDeviceRouteOptions = createMemo<SelectOption[]>(() => {
    const delays = app().delays()
    const base: SelectOption[] = [
      { value: 'pool', label: '默认代理', tone: 'cobalt' },
      { value: 'block_overseas', label: '禁止走代理（仅允许直连）', tone: 'amber' },
      { value: 'direct', label: '全量直连', tone: 'default' },
    ]

    for (const node of manualNodes()) {
      const delay = delays[node.tag]
      base.push({
        value: `dedicated:${node.tag}`,
        label: node.tag,
        group: '手动节点',
        badge: delay !== undefined ? formatDelay(delay) : node.node_type.toUpperCase(),
        tone: 'emerald',
      })
    }

    for (const tag of extraPoolNodes()) {
      const delay = delays[tag]
      base.push({
        value: `dedicated:${tag}`,
        label: tag,
        group: '订阅节点',
        badge: delay !== undefined ? formatDelay(delay) : undefined,
        tone: 'cobalt',
      })
    }

    return base
  })

  const buildRowRouteOptions = (row: TerminalRow): SelectOption[] => {
    const options = baseDeviceRouteOptions().map((opt) => {
      if (row.failClosed && row.target && opt.value === `dedicated:${row.target}`) {
        return { ...opt, badge: '已阻断', tone: 'crimson' as const }
      }
      return opt
    })

    if (row.mode === 'dedicated' && row.target) {
      const encoded = `dedicated:${row.target}`
      if (!options.some((opt) => opt.value === encoded)) {
        options.push({
          value: encoded,
          label: `${cleanNodeDisplay(row.target).name}（节点不可用）`,
          group: '不可用节点',
          badge: '已阻断',
          tone: 'crimson',
        })
      }
    }

    return options
  }

  const dialogDeviceAssignOptions = createMemo<SelectOption[]>(() => {
    const keepLabel = ipLocked() ? '保持当前策略不变' : '默认（跟随默认代理）'
    return [
      { value: 'keep', label: keepLabel, tone: 'default' },
      ...baseDeviceRouteOptions(),
    ]
  })

  const portOutboundOptions = createMemo<SelectOption[]>(() => {
    const delays = app().delays()
    const options: SelectOption[] = []

    for (const node of manualNodes()) {
      const delay = delays[node.tag]
      const proto = node.detour
        ? `${node.node_type.toUpperCase()} · 跳板`
        : node.node_type.toUpperCase()
      options.push({
        value: node.tag,
        label: node.tag,
        group: '手动节点',
        badge: delay !== undefined ? formatDelay(delay) : proto,
        tone: 'emerald',
      })
    }

    for (const tag of extraPoolNodes()) {
      const delay = delays[tag]
      options.push({
        value: tag,
        label: tag,
        group: '订阅节点',
        badge: delay !== undefined ? formatDelay(delay) : undefined,
        tone: 'cobalt',
      })
    }

    options.push(
      { value: 'proxy', label: '默认代理（动态选路）', group: '内置出口', tone: 'cobalt' },
      { value: 'direct', label: '本地直连', group: '内置出口', tone: 'default' },
    )

    const current = portOutbound().trim()
    if (current && !options.some((opt) => opt.value === current)) {
      options.unshift({
        value: current,
        label: `${cleanNodeDisplay(current).name}（节点不可用）`,
        group: '当前绑定',
        badge: '离线',
        tone: 'crimson',
      })
    }

    return options
  })

  const customRuleTargetOptions = createMemo<SelectOption[]>(() => {
    const delays = app().delays()
    const manualOpts: SelectOption[] = manualNodes().map((node) => ({
      value: node.tag,
      label: node.tag,
      group: '手动节点',
      badge: delays[node.tag] != null ? formatDelay(delays[node.tag]) : node.node_type.toUpperCase(),
      tone: 'emerald',
    }))
    const subOpts: SelectOption[] = extraPoolNodes().map((tag) => ({
      value: tag,
      label: tag,
      group: '订阅节点',
      badge: delays[tag] != null ? formatDelay(delays[tag]) : undefined,
      tone: 'cobalt',
    }))
    return [
      { value: 'proxy', label: '默认代理', tone: 'cobalt' },
      { value: 'direct', label: '直连', tone: 'emerald' },
      { value: 'reject', label: '拦截', tone: 'crimson' },
      ...manualOpts,
      ...subOpts,
    ]
  })

  const fieldOptions = createMemo(() => ruleFieldOptions(app().status().platform))
  const customRuleFieldOptions = createMemo<SelectOption[]>(() =>
    fieldOptions().map((opt) => ({
      value: opt.value,
      label: opt.label,
    })),
  )

  const protocolSelectOptions: SelectOption[] = PROTOCOL_OPTIONS.map((opt) => ({
    value: opt.value,
    label: opt.label.toUpperCase(),
  }))

  const fieldPlaceholder = createMemo(
    () => fieldOptions().find((opt) => opt.value === ruleField())?.placeholder || '请输入匹配值',
  )

  const handleRuleFieldChange = (nextField: string) => {
    setRuleField(nextField)
    if (nextField === 'protocol') {
      if (!PROTOCOL_OPTIONS.some((p) => p.value === ruleValue())) {
        setRuleValue('quic')
      }
    } else if (PROTOCOL_OPTIONS.some((p) => p.value === ruleValue())) {
      setRuleValue('')
    }
  }

  const applyPreset = (preset: RulePreset) => {
    setRuleField(preset.field)
    setRuleValue(preset.value)
    setRuleTarget(preset.recommendedTarget)
  }

  const submitCustomRule = async (event?: Event) => {
    event?.preventDefault()
    if (savingRule() || !ruleValue().trim()) return
    setSavingRule(true)
    try {
      const ok = await app().handleAddRule({
        field: ruleField(),
        value: ruleValue().trim(),
        target: ruleTarget(),
      })
      if (ok) {
        setRuleValue('')
        setRuleDrawerOpen(false)
      }
    } finally {
      setSavingRule(false)
    }
  }

  const applySeatStrategy = async (ip: string, rawValue: string) => {
    if (rawValue.startsWith('dedicated:')) {
      const target = rawValue.slice('dedicated:'.length)
      return app().handleAssignTerminal({ ip, mode: 'dedicated', target })
    }
    const mode = rawValue as TerminalAssignMode
    return app().handleAssignTerminal({ ip, mode })
  }

  const openCreateDeviceDialog = () => {
    setEditingIp('192.168.')
    setIpLocked(false)
    setFormName('')
    setFormDept('')
    setFormNote('')
    setInitialAssign('keep')
    setFormAssign('keep')
    setDeviceDialogOpen(true)
  }

  const openEditDeviceDialog = (row: TerminalRow) => {
    const currentAssign = encodeSeatSelectValue(row.mode, row.target)
    setEditingIp(row.ip)
    setIpLocked(true)
    setFormName(row.name || row.dhcpHostname || '')
    setFormDept(row.department || '')
    setFormNote(row.note || '')
    setInitialAssign(currentAssign)
    setFormAssign(currentAssign)
    setDeviceDialogOpen(true)
  }

  const submitDeviceDialog = async () => {
    if (savingDevice()) return
    const ip = editingIp().trim()
    const name = formName().trim()
    if (!ip || !name) return

    setSavingDevice(true)
    try {
      const ok = await app().handleUpsertTerminal({
        ip,
        name,
        department: formDept().trim() || undefined,
        note: formNote().trim() || undefined,
      })
      if (!ok) return

      const chosen = formAssign()
      if (chosen !== 'keep' && (!ipLocked() || chosen !== initialAssign())) {
        await applySeatStrategy(ip, chosen)
      }
      setDeviceDialogOpen(false)
    } finally {
      setSavingDevice(false)
    }
  }

  const openCreatePortDialog = () => {
    const nextPort = suggestNextAccessPort(app().accessPorts())
    const defaultOutbound = manualNodes()[0]?.tag || extraPoolNodes()[0] || 'proxy'
    setEditPortId(undefined)
    setPortName('')
    setPortNumber(nextPort)
    setPortOutbound(defaultOutbound)
    setPortUsername('')
    setPortPassword('')
    setPortDialogOpen(true)
  }

  const openEditPortDialog = (row: AccessPortRow) => {
    setEditPortId(row.id)
    setPortName(row.name)
    setPortNumber(row.port)
    setPortOutbound(row.outbound)
    setPortUsername(row.username || '')
    setPortPassword(row.password || '')
    setPortDialogOpen(true)
  }

  const authMismatch = () => Boolean(portUsername()) !== Boolean(portPassword())

  const submitPortDialog = async () => {
    if (savingPort() || authMismatch()) return
    setSavingPort(true)
    try {
      const ok = await app().handleSaveAccessPort({
        id: editPortId(),
        name: portName().trim(),
        port: Number(portNumber()),
        outbound: portOutbound().trim(),
        username: portUsername() || undefined,
        password: portPassword() || undefined,
      })
      if (ok) setPortDialogOpen(false)
    } finally {
      setSavingPort(false)
    }
  }

  const copyText = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value)
      app().showToast(`已复制${label}：${value}`, 'success')
    } catch {
      app().showToast(`请手动复制：${value}`, 'info')
    }
  }

  const searchAriaLabel = () => (
    activeTab() === 'devices'
      ? '搜索局域网设备'
      : activeTab() === 'ports'
        ? '搜索独立端口'
        : '搜索规则'
  )

  const searchPlaceholder = () => (
    activeTab() === 'devices'
      ? '搜索名称、标签、IP 或 MAC…'
      : activeTab() === 'ports'
        ? '搜索端口名称、端口号或节点…'
        : '搜索域名、IP 或出口…'
  )

  return (
    <>
      <header class="page-header">
        <div>
          <h1 class="page-title">分流隔离</h1>
          <p class="page-subtitle">按局域网设备、本地独立端口或匹配规则绑定专属流量出口</p>
        </div>
        <div class="page-actions">
          <Show when={activeTab() === 'devices' && app().terminalsSupported()}>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={openCreateDeviceDialog}
            >
              登记设备
            </Button>
          </Show>

          <Show when={activeTab() === 'ports' && app().accessPortsSupported()}>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={openCreatePortDialog}
            >
              新建端口
            </Button>
          </Show>

          <Show when={activeTab() === 'rules'}>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={() => setRuleDrawerOpen(true)}
            >
              新建规则
            </Button>
          </Show>
        </div>
      </header>

      <div class="studio-toolbar">
        <div class="seg-rail" role="group" aria-label="分流隔离维度">
          <button
            type="button"
            class={['seg-btn', { active: activeTab() === 'devices' }]}
            aria-pressed={activeTab() === 'devices' ? 'true' : 'false'}
            onClick={() => switchTab('devices')}
          >
            <span>局域网设备 ({terminalRows().length})</span>
            <Show when={failClosedSeatCount() > 0}>
              <span class="status-dot crimson" aria-hidden="true" />
            </Show>
          </button>

          <button
            type="button"
            class={['seg-btn', { active: activeTab() === 'ports' }]}
            aria-pressed={activeTab() === 'ports' ? 'true' : 'false'}
            onClick={() => switchTab('ports')}
          >
            <span>独立代理端口 ({accessPortRows().length})</span>
            <Show when={failClosedPortCount() > 0}>
              <span class="status-dot crimson" aria-hidden="true" />
            </Show>
          </button>

          <button
            type="button"
            class={['seg-btn', { active: activeTab() === 'rules' }]}
            aria-pressed={activeTab() === 'rules' ? 'true' : 'false'}
            onClick={() => switchTab('rules')}
          >
            <span>自定义规则 ({generalRules().length})</span>
            <Show when={skippedGeneralRuleCount() > 0}>
              <span class="status-dot crimson" aria-hidden="true" />
            </Show>
          </button>
        </div>

        <div class="studio-toolbar-right">
          <Show when={activeTab() === 'devices'}>
            <SelectPopover
              ariaLabel="按出口策略筛选"
              size="sm"
              align="right"
              value={deviceFilter()}
              options={DEVICE_FILTER_OPTIONS}
              searchableThreshold={10}
              onChange={(val) => setDeviceFilter(val as DeviceFilter)}
            />
          </Show>

          <Show when={activeTab() === 'rules' && deviceRuleCount() > 0}>
            <SelectPopover
              ariaLabel="规则显示范围"
              size="sm"
              align="right"
              value={ruleScope()}
              options={ruleScopeOptions()}
              searchableThreshold={10}
              onChange={(val) => setRuleScope(val as RuleScopeFilter)}
            />
          </Show>

          <div class="studio-search">
            <SearchIcon size={ICON.sm} />
            <input
              type="search"
              aria-label={searchAriaLabel()}
              placeholder={searchPlaceholder()}
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
            />
          </div>
        </div>
      </div>

      {/* Graceful Read-Only Fallback Banners for Older Backends */}
      <Show when={activeTab() === 'devices' && !app().terminalsSupported()}>
        <div class="circuit-banner warning" role="status">
          <div class="circuit-banner-main">
            <span class="status-dot amber" aria-hidden="true" />
            <span>
              当前后端处于只读兼容模式：已从活动连接与分流规则识别设备，更新 Harbor 后可保存设备备注并在线切换出口策略。
            </span>
          </div>
        </div>
      </Show>

      <Show when={activeTab() === 'ports' && !app().accessPortsSupported()}>
        <div class="circuit-banner warning" role="status">
          <div class="circuit-banner-main">
            <span class="status-dot amber" aria-hidden="true" />
            <span>
              当前后端版本暂不支持独立代理端口（HTTP / SOCKS5 混合监听），更新 Harbor 后即可启用。
            </span>
          </div>
        </div>
      </Show>

      {/* Tab 1: LAN Devices */}
      <Show when={activeTab() === 'devices'}>
        <section class="surface-box" aria-label="局域网设备列表">
          <Show
            when={filteredDevices().length > 0}
            fallback={
              <div class="empty-state">
                暂无匹配的局域网设备。当设备通过网关产生流量、存在 DHCP 租约或手动登记固定 IP 后将显示在此处。
              </div>
            }
          >
            <div class="hairline-list">
              <For each={filteredDevices()} keyed={(row) => row.ip}>
                {(row) => {
                  const assigning = () => app().pendingActions().has(`assignTerminal:${row().ip}`)

                  return (
                    <div class={['hairline-row', { 'danger-tint': row().failClosed }]}>
                      <div class="row-main">
                        <div class="row-title">
                          <span class={['status-dot', deviceDotTone(row())]} aria-hidden="true" />
                          <span>{row().displayName}</span>
                          <Show when={row().department}>
                            {(dept) => <span class="mono-tag">{dept()}</span>}
                          </Show>
                        </div>

                        <div class="row-sub mono">
                          {[
                            row().displayName !== row().ip ? row().ip : null,
                            row().dhcpHostname && row().dhcpHostname !== row().displayName ? row().dhcpHostname : null,
                            row().mac || null,
                            row().note || null,
                          ]
                            .filter(Boolean)
                            .join(' · ') || '局域网客户端'}
                        </div>
                      </div>

                      <div class="row-end">
                        <span class="row-meta num" style={{ 'white-space': 'nowrap' }}>
                          ↓ {formatSpeed(row().downloadSpeed)} / ↑ {formatSpeed(row().uploadSpeed)}
                        </span>

                        <div class="endpoint-picker-wrap">
                          <SelectPopover
                            ariaLabel={`设置 ${row().displayName} (${row().ip}) 出口策略`}
                            value={encodeSeatSelectValue(row().mode, row().target)}
                            options={buildRowRouteOptions(row())}
                            disabled={!app().terminalsSupported() || assigning()}
                            onChange={(val) => {
                              void applySeatStrategy(row().ip, val)
                            }}
                            searchPlaceholder="搜索节点…"
                            align="right"
                            size="sm"
                            fullWidth
                          />
                        </div>

                        <Show when={app().terminalsSupported()}>
                          <button
                            type="button"
                            class="icon-button"
                            title="编辑设备备注"
                            aria-label={`编辑设备 ${row().ip} 备注`}
                            onClick={() => openEditDeviceDialog(row())}
                          >
                            <EditIcon size={ICON.sm} />
                          </button>

                          <Show when={row().hasSavedProfile}>
                            <button
                              type="button"
                              class="icon-button danger"
                              title="清除已保存备注"
                              aria-label={`清除设备 ${row().ip} 备注`}
                              onClick={() => { void app().handleDeleteTerminal(row().ip) }}
                            >
                              <Trash2Icon size={ICON.sm} />
                            </button>
                          </Show>
                        </Show>
                      </div>
                    </div>
                  )
                }}
              </For>
            </div>
          </Show>
        </section>
      </Show>

      {/* Tab 2: Independent Local Proxy Ports */}
      <Show when={activeTab() === 'ports'}>
        <section class="surface-box" aria-label="独立代理端口列表">
          <div class="surface-note">
            <span>
              每个独立端口同时监听 HTTP 与 SOCKS5，可将不同浏览器或业务客户端固定绑定到指定出口（节点离线自动熔断）。
            </span>
            <Show when={manualNodes().length === 0}>
              <Button
                tone="secondary"
                size="sm"
                onClick={() => app().openNodeModal('socks')}
              >
                添加 SOCKS5 节点
              </Button>
            </Show>
          </div>

          <Show
            when={filteredPorts().length > 0}
            fallback={
              <div class="empty-state">
                暂未创建独立代理端口，点击右上角「新建端口」即可开放本地 HTTP / SOCKS5 监听入口。
              </div>
            }
          >
            <div class="hairline-list">
              <For each={filteredPorts()} keyed={(row) => row.id}>
                {(row) => {
                  const proxyCopyValue = () => (
                    row().has_auth && row().username && row().password
                      ? `${row().username}:${row().password}@${row().listenEndpoint}`
                      : row().listenEndpoint
                  )
                  const cleanedOutbound = () => cleanNodeDisplay(row().outbound)

                  return (
                    <div class={['hairline-row', { 'danger-tint': row().blocked }]}>
                      <div class="row-main">
                        <div class="row-title">
                          <span>{row().name}</span>
                          <button
                            type="button"
                            class="endpoint-code-pill mono"
                            title="点击复制代理地址"
                            aria-label={`复制 ${row().name} 代理地址 ${row().listenEndpoint}`}
                            onClick={() => { void copyText(proxyCopyValue(), '代理地址') }}
                          >
                            <span>{row().listenEndpoint}</span>
                            <CopyIcon size={12} />
                          </button>
                          <Show when={row().has_auth}>
                            <span class="mono-tag">账号认证</span>
                          </Show>
                        </div>

                        <div class="row-sub">
                          HTTP / SOCKS5 混合监听
                          {row().detourRelay ? ` · 前置跳板: ${cleanNodeDisplay(row().detourRelay!).name}` : ''}
                        </div>
                      </div>

                      <div class="row-end">
                        <Show
                          when={!row().blocked}
                          fallback={
                            <span class="status-inline danger">
                              <span class="status-dot crimson" aria-hidden="true" />
                              <span>节点不可用 · 已阻断 ({cleanedOutbound().name})</span>
                            </span>
                          }
                        >
                          <span class="status-inline">
                            <span class="status-dot emerald" aria-hidden="true" />
                            <Show when={cleanedOutbound().code}>
                              {(code) => <span class="mono-tag">{code()}</span>}
                            </Show>
                            <span>
                              {row().outbound === 'proxy'
                                ? '默认代理'
                                : row().outbound === 'direct'
                                  ? '本地直连'
                                  : cleanedOutbound().name}
                            </span>
                          </span>
                        </Show>

                        <span class="row-meta num" style={{ 'white-space': 'nowrap' }}>
                          ↓ {formatSpeed(row().downloadSpeed)} · {row().connectionCount} 连接
                        </span>

                        <Show when={app().accessPortsSupported()}>
                          <button
                            type="button"
                            class="icon-button"
                            title="编辑独立端口"
                            aria-label={`编辑独立端口 ${row().name}`}
                            onClick={() => openEditPortDialog(row())}
                          >
                            <EditIcon size={ICON.sm} />
                          </button>

                          <button
                            type="button"
                            class="icon-button danger"
                            title="删除独立端口"
                            aria-label={`删除独立端口 ${row().name}`}
                            onClick={() => app().handleOpenDeleteAccessPortConfirm(row())}
                          >
                            <Trash2Icon size={ICON.sm} />
                          </button>
                        </Show>
                      </div>
                    </div>
                  )
                }}
              </For>
            </div>
          </Show>
        </section>
      </Show>

      {/* Tab 3: Custom Routing Rules */}
      <Show when={activeTab() === 'rules'}>
        <section class="surface-box" aria-label="自定义规则列表">
          <div class="surface-note">
            <span>
              匹配顺序：独立代理端口 → 内网与大陆直连（规则分流模式）→ 设备与自定义规则 → 默认代理出口
            </span>
          </div>

          <Show
            when={visibleRules().length > 0}
            fallback={
              <div class="empty-state">
                {app().rules().length === 0
                  ? '暂无自定义分流规则，点击右上角「新建规则」添加。'
                  : ruleScope() === 'general' && deviceRuleCount() > 0 && !query().trim()
                    ? `当前仅有 ${deviceRuleCount()} 条局域网设备绑定规则，可在「局域网设备」标签页集中管理。`
                    : '没有匹配当前筛选条件的规则。'}
              </div>
            }
          >
            <div class="hairline-list">
              <For each={visibleRules()} keyed={(rule) => `${rule.index}:${rule.raw}`}>
                {(rule) => {
                  const targetState = () => describeRuleTarget(rule())
                  return (
                    <div class={['hairline-row', { 'danger-tint': Boolean(rule().skipped) }]}>
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
                        <span class={['status-inline', { danger: Boolean(rule().skipped) }]}>
                          <span class={['status-dot', targetState().tone]} />
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

      {/* Dialog 1: Register / Edit LAN Device */}
      <Show when={deviceDialogOpen()}>
        <Portal>
          <div class="modal-overlay drawer-overlay" onClick={() => setDeviceDialogOpen(false)}>
            <div
              ref={deviceDialogRef}
              class="modal-card drawer-panel"
              role="dialog"
              aria-modal="true"
              aria-labelledby={deviceTitleId}
              tabindex={-1}
              onClick={(e) => e.stopPropagation()}
            >
              <div class="modal-title-row">
                <div class="modal-title-wrap">
                  <MonitorSmartphoneIcon size={ICON.lg} class="icon-accent" />
                  <h3 id={deviceTitleId}>
                    {ipLocked() ? `编辑设备备注 · ${editingIp()}` : '登记局域网设备'}
                  </h3>
                </div>
                <button
                  type="button"
                  class="icon-button"
                  aria-label="关闭设备备注窗口"
                  onClick={() => setDeviceDialogOpen(false)}
                >
                  <XIcon size={ICON.md} />
                </button>
              </div>

              <div class="form-grid single">
                <label class="field">
                  <span>局域网 IPv4 地址</span>
                  <input
                    value={editingIp()}
                    disabled={ipLocked() || savingDevice()}
                    placeholder="例如：192.168.1.50"
                    onInput={(e) => setEditingIp(e.currentTarget.value)}
                  />
                </label>
              </div>

              <div class="form-grid two">
                <label class="field">
                  <span>设备名称</span>
                  <input
                    data-autofocus
                    value={formName()}
                    disabled={savingDevice()}
                    placeholder="例如：MacBook Pro / NAS"
                    onInput={(e) => setFormName(e.currentTarget.value)}
                  />
                </label>

                <label class="field">
                  <span>分组标签（可选）</span>
                  <input
                    value={formDept()}
                    disabled={savingDevice()}
                    placeholder="例如：书房 / 测试机"
                    onInput={(e) => setFormDept(e.currentTarget.value)}
                  />
                </label>
              </div>

              <div class="form-grid single">
                <div class="field">
                  <span>出口策略</span>
                  <SelectPopover
                    ariaLabel="设备出口策略"
                    value={formAssign()}
                    options={dialogDeviceAssignOptions()}
                    disabled={savingDevice()}
                    onChange={setFormAssign}
                    searchPlaceholder="搜索节点…"
                    fullWidth
                  />
                </div>
              </div>

              <div class="form-grid single">
                <label class="field">
                  <span>备注说明（可选）</span>
                  <input
                    value={formNote()}
                    disabled={savingDevice()}
                    placeholder="可选说明"
                    onInput={(e) => setFormNote(e.currentTarget.value)}
                  />
                </label>
              </div>

              <div class="modal-actions">
                <Button tone="ghost" size="sm" onClick={() => setDeviceDialogOpen(false)}>
                  取消
                </Button>
                <Button
                  tone="primary"
                  size="sm"
                  loading={savingDevice()}
                  disabled={!editingIp().trim() || !formName().trim()}
                  onClick={() => { void submitDeviceDialog() }}
                >
                  保存
                </Button>
              </div>
            </div>
          </div>
        </Portal>
      </Show>

      {/* Dialog 2: Create / Edit Independent Proxy Port */}
      <Show when={portDialogOpen()}>
        <Portal>
          <div class="modal-overlay drawer-overlay" onClick={() => setPortDialogOpen(false)}>
            <div
              ref={portDialogRef}
              class="modal-card drawer-panel"
              role="dialog"
              aria-modal="true"
              aria-labelledby={portTitleId}
              tabindex={-1}
              onClick={(e) => e.stopPropagation()}
            >
              <div class="modal-title-row">
                <div class="modal-title-wrap">
                  <NetworkIcon size={ICON.lg} class="icon-accent" />
                  <h3 id={portTitleId}>
                    {editPortId() ? '编辑独立代理端口' : '新建独立代理端口'}
                  </h3>
                </div>
                <button
                  type="button"
                  class="icon-button"
                  aria-label="关闭独立端口窗口"
                  onClick={() => setPortDialogOpen(false)}
                >
                  <XIcon size={ICON.md} />
                </button>
              </div>

              <div class="form-grid two">
                <label class="field">
                  <span>端口名称</span>
                  <input
                    data-autofocus
                    placeholder="例如：独立浏览器 01"
                    value={portName()}
                    disabled={savingPort()}
                    onInput={(e) => setPortName(e.currentTarget.value)}
                  />
                </label>

                <label class="field">
                  <span>监听端口</span>
                  <input
                    type="number"
                    min={1024}
                    max={65535}
                    value={portNumber()}
                    disabled={savingPort()}
                    onInput={(e) => setPortNumber(Number(e.currentTarget.value || 0))}
                  />
                </label>
              </div>

              <div class="form-grid single">
                <div class="field">
                  <span>绑定出口</span>
                  <SelectPopover
                    ariaLabel="绑定出口"
                    value={portOutbound()}
                    options={portOutboundOptions()}
                    disabled={savingPort()}
                    onChange={setPortOutbound}
                    searchPlaceholder="搜索节点…"
                    fullWidth
                  />
                </div>
              </div>

              <div class="form-grid two">
                <label class="field">
                  <span>认证用户名（可选）</span>
                  <input
                    autocomplete="off"
                    placeholder="留空则局域网内免密连接"
                    value={portUsername()}
                    disabled={savingPort()}
                    onInput={(e) => setPortUsername(e.currentTarget.value)}
                  />
                </label>

                <label class="field">
                  <span>认证密码（可选）</span>
                  <input
                    type="password"
                    autocomplete="new-password"
                    placeholder="与用户名同时填写或留空"
                    value={portPassword()}
                    disabled={savingPort()}
                    onInput={(e) => setPortPassword(e.currentTarget.value)}
                  />
                </label>
              </div>

              <Show when={authMismatch()}>
                <p class="field-error" role="alert">认证用户名与密码需同时填写或同时留空。</p>
              </Show>

              <div class="modal-actions">
                <Button tone="ghost" size="sm" onClick={() => setPortDialogOpen(false)}>
                  取消
                </Button>
                <Button
                  tone="primary"
                  size="sm"
                  loading={savingPort()}
                  disabled={!portName().trim() || portNumber() < 1024 || portNumber() > 65535 || !portOutbound() || authMismatch()}
                  onClick={() => { void submitPortDialog() }}
                >
                  {editPortId() ? '保存修改' : '创建端口'}
                </Button>
              </div>
            </div>
          </div>
        </Portal>
      </Show>

      {/* Dialog 3: Create Custom Rule */}
      <Show when={ruleDrawerOpen()}>
        <Portal>
          <div class="modal-overlay drawer-overlay" onClick={() => setRuleDrawerOpen(false)}>
            <form
              ref={ruleDialogRef}
              class="modal-card drawer-panel"
              role="dialog"
              aria-modal="true"
              aria-labelledby={ruleTitleId}
              tabindex={-1}
              onClick={(e) => e.stopPropagation()}
              onSubmit={(e) => { void submitCustomRule(e) }}
            >
              <div class="modal-title-row">
                <div class="modal-title-wrap">
                  <RouteIcon size={ICON.lg} class="icon-accent" />
                  <h3 id={ruleTitleId}>新建分流规则</h3>
                </div>
                <button
                  type="button"
                  class="icon-button"
                  aria-label="关闭新建规则窗口"
                  onClick={() => setRuleDrawerOpen(false)}
                >
                  <XIcon size={ICON.md} />
                </button>
              </div>

              <div class="field">
                <span>常用模板（可选）</span>
                <div class="preset-chips">
                  <For each={RULE_PRESETS}>
                    {(preset) => (
                      <button
                        type="button"
                        class={[
                          'preset-chip',
                          { active: ruleField() === preset.field && ruleValue() === preset.value },
                        ]}
                        title={preset.hint}
                        onClick={() => applyPreset(preset)}
                      >
                        {preset.label}
                      </button>
                    )}
                  </For>
                </div>
              </div>

              <div class="form-grid single">
                <div class="field">
                  <span>匹配类型</span>
                  <SelectPopover
                    ariaLabel="匹配类型"
                    fullWidth
                    value={ruleField()}
                    options={customRuleFieldOptions()}
                    searchableThreshold={12}
                    onChange={handleRuleFieldChange}
                  />
                </div>
              </div>

              <div class="form-grid single">
                <Show
                  when={ruleField() === 'protocol'}
                  fallback={
                    <label class="field">
                      <span>匹配值</span>
                      <input
                        data-autofocus
                        aria-label="匹配值"
                        placeholder={fieldPlaceholder()}
                        value={ruleValue()}
                        onInput={(e) => setRuleValue(e.currentTarget.value)}
                      />
                    </label>
                  }
                >
                  <div class="field">
                    <span>嗅探协议</span>
                    <SelectPopover
                      ariaLabel="嗅探协议"
                      fullWidth
                      value={ruleValue() || 'quic'}
                      options={protocolSelectOptions}
                      searchableThreshold={12}
                      onChange={setRuleValue}
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
                    value={ruleTarget()}
                    options={customRuleTargetOptions()}
                    searchPlaceholder="搜索节点…"
                    onChange={setRuleTarget}
                  />
                </div>
              </div>

              <div class="modal-actions">
                <Button tone="ghost" size="sm" onClick={() => setRuleDrawerOpen(false)}>
                  取消
                </Button>
                <Button
                  type="submit"
                  tone="primary"
                  size="sm"
                  loading={savingRule()}
                  disabled={!ruleValue().trim()}
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
