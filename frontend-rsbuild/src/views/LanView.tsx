import { For, Show, createEffect, createMemo, createSignal, createUniqueId, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { SelectPopover, cleanNodeDisplay, type SelectOption } from '../components/SelectPopover'
import { useDialog } from '../hooks/useDialog'
import { classNames, formatDelay, formatSpeed } from '../utils'
import { buildTerminalRows, type TerminalRow } from '../domain/terminals'
import { buildAccessPortRows, suggestNextAccessPort, type AccessPortRow } from '../domain/accessPorts'
import type { TerminalAssignMode } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import {
  CopyIcon,
  EditIcon,
  MonitorSmartphoneIcon,
  NetworkIcon,
  PlusIcon,
  SearchIcon,
  Trash2Icon,
  XIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

type LanSection = 'devices' | 'ports'
type DeviceFilter = 'all' | 'dedicated' | 'pool' | 'block_overseas' | 'fail_closed'

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

export function LanView(props: {
  app: AppController
  initialSection?: LanSection
}): JSX.Element {
  const app = () => props.app

  const pickInitialSection = (): LanSection => untrack(() => {
    if (props.initialSection) return props.initialSection
    const hasBlockedPort = app().accessPorts().some((port) => port.blocked)
    const hasFailClosedSeat = buildTerminalRows({
      terminals: app().terminals(),
      rules: app().rules(),
      connections: app().connectionsInfo().connections,
    }).some((row) => row.failClosed)
    if (hasBlockedPort && !hasFailClosedSeat) return 'ports'
    return 'devices'
  })

  const [section, setSection] = createSignal<LanSection>(pickInitialSection())
  const [deviceFilter, setDeviceFilter] = createSignal<DeviceFilter>('all')
  const [query, setQuery] = createSignal('')

  createEffect(
    () => props.initialSection,
    (next) => {
      if (next) setSection(next)
    },
  )

  // Device profile & optional assignment dialog state
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

  // Independent proxy port dialog state
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

  const failClosedSeatCount = createMemo(() => terminalRows().filter((row) => row.failClosed).length)
  const failClosedPortCount = createMemo(() => accessPortRows().filter((row) => row.blocked).length)

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
      ].some((field) => field.toLowerCase().includes(q))
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
      ].some((field) => field.toLowerCase().includes(q))
    ))
  })

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

  return (
    <>
      <header class="page-header">
        <div>
          <h1 class="page-title">设备与端口</h1>
          <p class="page-subtitle">为局域网设备分配独立出口策略，或创建绑定固定节点的本地代理端口</p>
        </div>
        <div class="page-actions">
          <Show when={section() === 'devices' && app().terminalsSupported()}>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={openCreateDeviceDialog}
            >
              登记设备
            </Button>
          </Show>

          <Show when={section() === 'ports' && app().accessPortsSupported()}>
            <Button
              tone="primary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={openCreatePortDialog}
            >
              新建端口
            </Button>
          </Show>
        </div>
      </header>

      <div class="studio-toolbar">
        <div class="seg-rail" role="group" aria-label="局域网与端口切换">
          <button
            type="button"
            class={classNames('seg-btn', section() === 'devices' && 'active')}
            aria-pressed={section() === 'devices' ? 'true' : 'false'}
            onClick={() => setSection('devices')}
          >
            <span>局域网设备 ({terminalRows().length})</span>
            <Show when={failClosedSeatCount() > 0}>
              <span class="status-dot crimson" aria-hidden="true" />
            </Show>
          </button>

          <button
            type="button"
            class={classNames('seg-btn', section() === 'ports' && 'active')}
            aria-pressed={section() === 'ports' ? 'true' : 'false'}
            onClick={() => setSection('ports')}
          >
            <span>独立代理端口 ({accessPortRows().length})</span>
            <Show when={failClosedPortCount() > 0}>
              <span class="status-dot crimson" aria-hidden="true" />
            </Show>
          </button>
        </div>

        <div class="studio-toolbar-right">
          <Show when={section() === 'devices'}>
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

          <div class="studio-search">
            <SearchIcon size={ICON.sm} />
            <input
              type="search"
              aria-label={section() === 'devices' ? '搜索局域网设备' : '搜索独立端口'}
              placeholder={section() === 'devices' ? '搜索名称、标签、IP 或 MAC…' : '搜索端口名称、端口号或节点…'}
              value={query()}
              onInput={(e) => setQuery(e.currentTarget.value)}
            />
          </div>
        </div>
      </div>

      {/* Graceful Read-Only Fallback Banners for Older Backends */}
      <Show when={section() === 'devices' && !app().terminalsSupported()}>
        <div class="circuit-banner warning" role="status">
          <div class="circuit-banner-main">
            <span class="status-dot amber" aria-hidden="true" />
            <span>
              当前后端处于只读兼容模式：已从活动连接与分流规则识别设备，更新 Harbor 后可保存设备备注并在线切换出口策略。
            </span>
          </div>
        </div>
      </Show>

      <Show when={section() === 'ports' && !app().accessPortsSupported()}>
        <div class="circuit-banner warning" role="status">
          <div class="circuit-banner-main">
            <span class="status-dot amber" aria-hidden="true" />
            <span>
              当前后端版本暂不支持独立代理端口（HTTP / SOCKS5 混合监听），更新 Harbor 后即可启用。
            </span>
          </div>
        </div>
      </Show>

      {/* Section A: LAN Devices */}
      <Show when={section() === 'devices'}>
        <section class="surface-box" aria-label="局域网设备列表">
          <Show
            when={filteredDevices().length > 0}
            fallback={
              <div class="empty-state">
                暂无匹配的局域网设备。当设备通过本机/网关产生流量，或手动登记固定 IP 后会显示在此处。
              </div>
            }
          >
            <div class="hairline-list">
              <For each={filteredDevices()} keyed={(row) => row.ip}>
                {(row) => {
                  const assigning = () => app().pendingActions().has(`assignTerminal:${row().ip}`)

                  return (
                    <div class={classNames('hairline-row', row().failClosed && 'danger-tint')}>
                      <div class="row-main">
                        <div class="row-title">
                          <span class={classNames('status-dot', deviceDotTone(row()))} aria-hidden="true" />
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

      {/* Section B: Independent Local Proxy Ports */}
      <Show when={section() === 'ports'}>
        <section class="surface-box" aria-label="独立代理端口列表">
          <div class="surface-note">
            <span>
              每个独立端口同时支持 HTTP 与 SOCKS5 协议，可将不同应用或浏览器配置的流量固定绑定到指定出口节点。
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
                暂未创建独立代理端口，点击右上角「新建端口」即可添加本地 HTTP / SOCKS5 监听入口。
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
                    <div class={classNames('hairline-row', row().blocked && 'danger-tint')}>
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

      {/* Dialog 1: Register / Edit LAN Device */}
      <Show when={deviceDialogOpen()}>
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
      </Show>

      {/* Dialog 2: Create / Edit Independent Proxy Port */}
      <Show when={portDialogOpen()}>
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
      </Show>
    </>
  )
}
