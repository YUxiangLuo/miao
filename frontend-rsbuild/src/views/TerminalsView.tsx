import { For, Show, createMemo, createSignal, createUniqueId } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { useDialog } from '../hooks/useDialog'
import { classNames, formatSpeed } from '../utils'
import { buildTerminalRows, terminalModeLabel, type TerminalRow } from '../domain/terminals'
import type { TerminalAssignMode } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import {
  EditIcon,
  LockIcon,
  MonitorSmartphoneIcon,
  PlusIcon,
  SearchIcon,
  Trash2Icon,
  XIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

type TerminalFilter = 'all' | 'dedicated' | 'pool' | 'block_overseas' | 'fail_closed'

function encodeSeatSelectValue(mode: TerminalAssignMode, target?: string): string {
  if (mode === 'dedicated' && target) return `dedicated:${target}`
  return mode
}

export function TerminalsView(props: { app: AppController }) {
  const app = () => props.app
  const [query, setQuery] = createSignal('')
  const [filter, setFilter] = createSignal<TerminalFilter>('all')

  const [drawerOpen, setDrawerOpen] = createSignal(false)
  const [editingIp, setEditingIp] = createSignal('')
  const [ipLocked, setIpLocked] = createSignal(false)
  const [formName, setFormName] = createSignal('')
  const [formDept, setFormDept] = createSignal('')
  const [formNote, setFormNote] = createSignal('')
  const [saving, setSaving] = createSignal(false)

  const titleId = createUniqueId()
  const dialogRef = useDialog(drawerOpen, () => setDrawerOpen(false))

  const rows = createMemo(() => buildTerminalRows({
    terminals: app().terminals(),
    rules: app().rules(),
    connections: app().connectionsInfo().connections,
  }))

  const filteredRows = createMemo(() => {
    const q = query().trim().toLowerCase()
    const f = filter()
    return rows().filter((row) => {
      if (f === 'dedicated' && row.mode !== 'dedicated') return false
      if (f === 'pool' && row.mode !== 'pool') return false
      if (f === 'block_overseas' && row.mode !== 'block_overseas') return false
      if (f === 'fail_closed' && !row.failClosed) return false
      if (!q) return true
      return [
        row.ip,
        row.displayName,
        row.department || '',
        row.dhcpHostname || '',
        row.mac || '',
        row.target || '',
      ].some((field) => field.toLowerCase().includes(q))
    })
  })

  const dedicatedTargets = createMemo(() => {
    const manual = app().nodes().map((n) => n.tag)
    const extras = app().ruleNodeNames().filter((name) => !manual.includes(name))
    return { manual, extras }
  })

  const openCreateDrawer = () => {
    setEditingIp('192.168.8.')
    setIpLocked(false)
    setFormName('')
    setFormDept('')
    setFormNote('')
    setDrawerOpen(true)
  }

  const openEditDrawer = (row: TerminalRow) => {
    setEditingIp(row.ip)
    setIpLocked(true)
    setFormName(row.name || row.dhcpHostname || '')
    setFormDept(row.department || '')
    setFormNote(row.note || '')
    setDrawerOpen(true)
  }

  const submitProfile = async () => {
    if (saving()) return
    setSaving(true)
    try {
      const ok = await app().handleUpsertTerminal({
        ip: editingIp().trim(),
        name: formName().trim(),
        department: formDept().trim() || undefined,
        note: formNote().trim() || undefined,
      })
      if (ok) setDrawerOpen(false)
    } finally {
      setSaving(false)
    }
  }

  const handleSeatStrategyChange = (ip: string, rawValue: string) => {
    if (rawValue.startsWith('dedicated:')) {
      const target = rawValue.slice('dedicated:'.length)
      void app().handleAssignTerminal({ ip, mode: 'dedicated', target })
      return
    }
    const mode = rawValue as TerminalAssignMode
    void app().handleAssignTerminal({ ip, mode })
  }

  return (
    <>
      <Show when={!app().terminalsSupported()}>
        <div class="hg-alert info" role="status">
          <div class="hg-alert-main">
            <MonitorSmartphoneIcon size={ICON.lg} />
            <div>
              <strong>当前连接的路由器正在运行旧版网关内核（只读兼容模式）</strong>
              <p>
                已自动从实时连接与现有路由规则中识别在线工位终端。将网关升级至最新 HarborGate 版本后，即可启用 DHCP 主机名自动采集、员工部门备注持久化及单事务无漏流工位切换。
              </p>
            </div>
          </div>
        </div>
      </Show>

      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <MonitorSmartphoneIcon size={ICON.md} class="icon-accent" />
              办公工位与业务手机整机路由分配
            </h2>
            <p>
              整机透明层策略：财务/行政可设「仅国内办公」，运营手机或单店专机可一键「锁定独享专线（离线即熔断）」，国内企微/飞书/ERP 始终保持本地直连。
            </p>
          </div>
          <div class="hg-card-actions">
            <Show when={app().terminalsSupported()}>
              <Button
                tone="primary"
                size="sm"
                icon={<PlusIcon size={ICON.xs} />}
                onClick={openCreateDrawer}
              >
                登记固定 IP 工位
              </Button>
            </Show>
          </div>
        </div>

        <div class="hg-card-body" style={{ 'padding-bottom': '12px' }}>
          <div class="hg-toolbar">
            <div class="hg-segmented" role="group" aria-label="工位筛选">
              {([
                ['all', `全部终端 (${rows().length})`],
                ['dedicated', '独享专线'],
                ['pool', '公共线路池'],
                ['block_overseas', '仅国内办公'],
                ['fail_closed', '离线熔断'],
              ] as const).map(([key, label]) => (
                <button
                  type="button"
                  class={classNames('hg-segment', filter() === key && 'active')}
                  aria-pressed={filter() === key ? 'true' : 'false'}
                  onClick={() => setFilter(key)}
                >
                  {label}
                </button>
              ))}
            </div>

            <div class="hg-search">
              <SearchIcon size={ICON.sm} />
              <input
                type="search"
                aria-label="搜索工位"
                placeholder="搜索员工名、部门、IP、主机名或 MAC…"
                value={query()}
                onInput={(e) => setQuery(e.currentTarget.value)}
              />
            </div>
          </div>
        </div>

        <Show
          when={filteredRows().length > 0}
          fallback={
            <div class="empty-block">
              没有匹配的办公终端。你可以点击右上角「登记固定 IP 工位」预先录入员工电脑或业务手机。
            </div>
          }
        >
          <div class="hg-table-wrap">
            <table class="hg-table">
              <thead>
                <tr>
                  <th>员工工位 / 部门</th>
                  <th>局域网 IP / 硬件标识</th>
                  <th>当前整机隔离状态</th>
                  <th>活跃外贸平台</th>
                  <th>实时吞吐</th>
                  <th>整机出口快速分配</th>
                </tr>
              </thead>
              <tbody>
                <For each={filteredRows()} keyed={(row) => row.ip}>
                  {(row) => {
                    const selectValue = () => encodeSeatSelectValue(row().mode, row().target)
                    const assigning = () => app().pendingActions().has(`assignTerminal:${row().ip}`)

                    return (
                      <tr class={classNames(row().failClosed && 'row-danger')}>
                        <td>
                          <div class="hg-entity-primary">
                            <span>{row().displayName}</span>
                            <Show when={row().department}>
                              {(dept) => <span class="badge ocean">{dept()}</span>}
                            </Show>
                          </div>
                          <Show when={row().note}>
                            {(note) => <div class="hg-entity-sub">{note()}</div>}
                          </Show>
                        </td>
                        <td>
                          <div class="mono" style={{ 'font-weight': '600' }}>{row().ip}</div>
                          <div class="hg-entity-sub mono">
                            {[
                              row().dhcpHostname ? `主机: ${row().dhcpHostname}` : null,
                              row().mac ? `MAC: ${row().mac}` : null,
                            ].filter(Boolean).join(' · ') || '静态登记终端'}
                          </div>
                        </td>
                        <td>
                          <Show
                            when={!row().failClosed}
                            fallback={
                              <span class="badge danger">
                                ⛔ 离线熔断保护中 ({row().target})
                              </span>
                            }
                          >
                            <span
                              class={classNames(
                                'badge',
                                row().mode === 'dedicated'
                                  ? 'emerald'
                                  : row().mode === 'block_overseas'
                                    ? 'amber'
                                    : row().mode === 'direct'
                                      ? 'slate'
                                      : 'ocean',
                              )}
                            >
                              {row().mode === 'dedicated' ? <LockIcon size={11} /> : null}
                              {row().mode === 'dedicated' && row().target
                                ? `独享专线 · ${row().target}`
                                : terminalModeLabel(row().mode)}
                            </span>
                          </Show>
                        </td>
                        <td>
                          <Show
                            when={row().activePlatforms.length > 0}
                            fallback={<span class="hg-entity-sub">—</span>}
                          >
                            <div class="hg-platform-tags">
                              <For each={row().activePlatforms} keyed={(p) => p.id}>
                                {(p) => <span class="badge slate">{p().name}</span>}
                              </For>
                            </div>
                          </Show>
                        </td>
                        <td class="num">
                          <div>↓ {formatSpeed(row().downloadSpeed)} / ↑ {formatSpeed(row().uploadSpeed)}</div>
                          <div class="hg-entity-sub">{row().connectionCount} 活跃连接</div>
                        </td>
                        <td>
                          <div class="hg-inline-actions">
                            <select
                              class="hg-select"
                              aria-label={`分配 ${row().displayName} (${row().ip}) 整机策略`}
                              value={selectValue()}
                              disabled={!app().terminalsSupported() || assigning()}
                              onChange={(e) => handleSeatStrategyChange(row().ip, e.currentTarget.value)}
                            >
                              <option value="pool">🌐 跟随公共出海线路池</option>
                              <option value="block_overseas">🏢 仅限国内办公 (阻断出海)</option>
                              <option value="direct">⚡ 全量直连 (不经代理)</option>
                              <Show when={dedicatedTargets().manual.length > 0}>
                                <optgroup label="🔒 锁定独享静态专线 (离线熔断)">
                                  <For each={dedicatedTargets().manual}>
                                    {(tag) => <option value={`dedicated:${tag}`}>🔒 独享 · {tag}</option>}
                                  </For>
                                </optgroup>
                              </Show>
                              <Show when={dedicatedTargets().extras.length > 0}>
                                <optgroup label="指定特定线路池节点">
                                  <For each={dedicatedTargets().extras}>
                                    {(tag) => <option value={`dedicated:${tag}`}>节点 · {tag}</option>}
                                  </For>
                                </optgroup>
                              </Show>
                              <Show
                                when={
                                  row().mode === 'dedicated'
                                  && row().target
                                  && !dedicatedTargets().manual.includes(row().target!)
                                  && !dedicatedTargets().extras.includes(row().target!)
                                }
                              >
                                <option value={`dedicated:${row().target}`}>
                                  ⛔ 离线专线 · {row().target}
                                </option>
                              </Show>
                            </select>

                            <Show when={app().terminalsSupported()}>
                              <button
                                type="button"
                                class="icon-button"
                                title="编辑员工/部门备注"
                                aria-label={`编辑工位 ${row().ip} 备注`}
                                onClick={() => openEditDrawer(row())}
                              >
                                <EditIcon size={ICON.sm} />
                              </button>
                              <Show when={row().hasSavedProfile}>
                                <button
                                  type="button"
                                  class="icon-button danger"
                                  title="清除备注档案"
                                  aria-label={`清除工位 ${row().ip} 备注`}
                                  onClick={() => { void app().handleDeleteTerminal(row().ip) }}
                                >
                                  <Trash2Icon size={ICON.sm} />
                                </button>
                              </Show>
                            </Show>
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

      <Show when={drawerOpen()}>
        <div class="modal-overlay drawer-overlay" onClick={() => setDrawerOpen(false)}>
          <div
            ref={dialogRef}
            class="modal-card drawer-panel"
            role="dialog"
            aria-modal="true"
            aria-labelledby={titleId}
            tabindex={-1}
            onClick={(e) => e.stopPropagation()}
          >
            <div class="modal-title-row">
              <div class="modal-title-wrap">
                <MonitorSmartphoneIcon size={ICON.lg} class="icon-accent" />
                <h3 id={titleId}>{ipLocked() ? `编辑工位档案 · ${editingIp()}` : '登记固定 IP 办公工位'}</h3>
              </div>
              <button
                type="button"
                class="icon-button"
                aria-label="关闭工位编辑抽屉"
                onClick={() => setDrawerOpen(false)}
              >
                <XIcon size={ICON.md} />
              </button>
            </div>

            <p class="drawer-help">
              工位备注保存在网关稳定配置中，修改姓名或部门不会中断当前正在进行的通话或上传连接。建议在路由器 DHCP 中为重要运营电脑绑定静态 IP。
            </p>

            <div class="form-grid single">
              <label class="field">
                <span>局域网 IPv4 地址</span>
                <input
                  value={editingIp()}
                  disabled={ipLocked() || saving()}
                  placeholder="例如：192.168.8.180"
                  onInput={(e) => setEditingIp(e.currentTarget.value)}
                />
              </label>
            </div>

            <div class="form-grid two">
              <label class="field">
                <span>员工 / 设备名称</span>
                <input
                  data-autofocus
                  value={formName()}
                  disabled={saving()}
                  placeholder="例如：李运营-美区台式机"
                  onInput={(e) => setFormName(e.currentTarget.value)}
                />
              </label>
              <label class="field">
                <span>所属业务组 / 部门（可选）</span>
                <input
                  value={formDept()}
                  disabled={saving()}
                  placeholder="例如：Amazon 美区组 / 财务部"
                  onInput={(e) => setFormDept(e.currentTarget.value)}
                />
              </label>
            </div>

            <div class="form-grid single">
              <label class="field">
                <span>工位用途备注（可选）</span>
                <input
                  value={formNote()}
                  disabled={saving()}
                  placeholder="例如：主跑美区一店 + WhatsApp 跟单手机"
                  onInput={(e) => setFormNote(e.currentTarget.value)}
                />
              </label>
            </div>

            <div class="modal-actions">
              <Button tone="ghost" size="sm" onClick={() => setDrawerOpen(false)}>取消</Button>
              <Button
                tone="primary"
                size="sm"
                loading={saving()}
                disabled={!editingIp().trim() || !formName().trim()}
                onClick={() => { void submitProfile() }}
              >
                保存工位档案
              </Button>
            </div>
          </div>
        </div>
      </Show>
    </>
  )
}
