import { For, Match, Show, Switch, createMemo, createSignal, onSettled } from 'solid-js'
import { ICON } from '../tokens'
import { Button, LogoIcon, ToastStack } from './ui'
import { ConfirmModal } from './ConfirmModal'
import { NodeModal } from './node-modal/NodeModal'
import { SubDetailModal } from './SubDetailModal'
import { ScheduleModal } from './ScheduleModal'
import { VpsTasks } from './VpsTasks'
import { CommandPalette, useCommandShortcut } from './CommandPalette'
import { buildTerminalRows, isSingleSeatRule } from '../domain/terminals'
import { buildAccessPortRows } from '../domain/accessPorts'
import { NodesView } from '../views/NodesView'
import { RoutingView } from '../views/RoutingView'
import { ConnectionsView } from '../views/ConnectionsView'
import { SettingsView } from '../views/SettingsView'
import { formatSpeed, formatUptime } from '../utils'
import type { ConsoleNavKey, RoutingTabKey } from '../hooks/useAppData'
import type { RouteMode, SubStatus } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import {
  ActivityIcon,
  GlobeIcon,
  PlayIcon,
  RouteIcon,
  SearchIcon,
  SettingsIcon,
  SquareIcon,
  type IconProps,
} from './icons'

type AppController = ReturnType<typeof useAppController>

function detectMobileViewport(): boolean {
  if (typeof window === 'undefined') return false
  if (window.innerWidth > 0 && window.innerWidth <= 820) return true
  return Boolean(window.matchMedia?.('(max-width: 820px)')?.matches)
}

export function AppShell(props: { app: AppController }) {
  const app = () => props.app
  const [cmdOpen, setCmdOpen] = createSignal(false)
  const [inspectingSub, setInspectingSub] = createSignal<SubStatus | null>(null)
  const [showScheduleModal, setShowScheduleModal] = createSignal(false)
  const [isMobile, setIsMobile] = createSignal(detectMobileViewport())

  useCommandShortcut(() => setCmdOpen((prev) => !prev))

  onSettled(() => {
    if (typeof window === 'undefined') return
    const syncViewport = () => setIsMobile(detectMobileViewport())
    window.addEventListener('resize', syncViewport)
    const mql = window.matchMedia?.('(max-width: 820px)')
    mql?.addEventListener?.('change', syncViewport)
    return () => {
      window.removeEventListener('resize', syncViewport)
      mql?.removeEventListener?.('change', syncViewport)
    }
  })

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

  const generalRules = createMemo(() => app().rules().filter((r) => !isSingleSeatRule(r)))

  const failClosedSeatCount = createMemo(() => terminalRows().filter((r) => r.failClosed).length)
  const failClosedPortCount = createMemo(() => accessPortRows().filter((r) => r.blocked).length)
  const skippedGeneralRuleCount = createMemo(() => generalRules().filter((r) => r.skipped).length)

  const routingAlertCount = createMemo(
    () => failClosedSeatCount() + failClosedPortCount() + skippedGeneralRuleCount(),
  )

  const firstBlockedRoutingTab = createMemo<RoutingTabKey>(() => {
    if (failClosedSeatCount() > 0) return 'devices'
    if (failClosedPortCount() > 0) return 'ports'
    return 'rules'
  })

  const nodesSourceHasAlert = createMemo(() => {
    if (app().subs().some((sub) => sub.state === 'failed')) return true
    const knownTargets = new Set(app().ruleNodeNames())
    return app().nodes().some((node) => Boolean(node.detour) && !knownTargets.has(node.detour!))
  })

  const statusTone = createMemo(() => {
    if (app().status().ready) return 'emerald pulse'
    if (app().status().initializing) return 'amber'
    return 'muted'
  })

  const statusText = createMemo(() => {
    if (app().status().ready) return '运行中'
    if (app().status().initializing) return '启动中'
    return '已停止'
  })

  const switchRouteMode = (mode: RouteMode) => {
    if (mode === app().status().route_mode) return
    app().handleOpenSetRouteModeConfirm(mode)
  }

  const navItems: ReadonlyArray<{
    key: ConsoleNavKey
    label: string
    shortLabel: string
    icon: (props: IconProps) => ReturnType<typeof GlobeIcon>
    badge?: () => string | undefined
    alertTone?: () => 'crimson' | 'emerald' | undefined
  }> = [
    {
      key: 'nodes',
      label: '代理节点',
      shortLabel: '节点',
      icon: GlobeIcon,
      badge: () => {
        const poolCount = app().primaryGroup()?.all?.length ?? 0
        if (poolCount > 0) return String(poolCount)
        const manualCount = app().nodes().length
        return manualCount > 0 ? String(manualCount) : undefined
      },
      alertTone: () => (nodesSourceHasAlert() ? 'crimson' : undefined),
    },
    {
      key: 'routing',
      label: '分流隔离',
      shortLabel: '分流',
      icon: RouteIcon,
      badge: () => {
        const total = terminalRows().length + accessPortRows().length + generalRules().length
        return total > 0 ? String(total) : undefined
      },
      alertTone: () => (routingAlertCount() > 0 ? 'crimson' : undefined),
    },
    {
      key: 'connections',
      label: '实时连接',
      shortLabel: '连接',
      icon: ActivityIcon,
      badge: () => {
        const count = app().connectionsInfo().connections.length
        return app().status().ready && count > 0 ? String(count) : undefined
      },
    },
    {
      key: 'settings',
      label: '系统设置',
      shortLabel: '设置',
      icon: SettingsIcon,
      alertTone: () => (app().versionInfo().has_update ? 'emerald' : undefined),
    },
  ]

  return (
    <div class="app-layout">
      {/* Desktop Left Sidebar */}
      <Show when={!isMobile()}>
        <aside class="app-sidebar" aria-label="侧边栏">
          <div class="sidebar-top">
            <div class="sidebar-brand">
              <div class="sidebar-brand-main">
                <LogoIcon size={26} />
                <span class="sidebar-brand-name">Harbor</span>
              </div>
              <Show when={app().versionInfo().current}>
                {(ver) => (
                  <span class="mono-tag">
                    v{ver().replace(/^v/, '')}
                  </span>
                )}
              </Show>
            </div>

            <button
              type="button"
              class="sidebar-cmd-btn"
              aria-label="打开命令面板"
              title="快速搜索或跳转 (⌘K)"
              onClick={() => setCmdOpen(true)}
            >
              <span class="sidebar-cmd-label">
                <SearchIcon size={13} />
                <span>搜索或跳转…</span>
              </span>
              <span class="kbd-chip">⌘K</span>
            </button>

            <nav class="sidebar-nav" role="navigation" aria-label="主导航">
              <For each={navItems}>
                {(item) => {
                  const IconComp = item.icon
                  return (
                    <button
                      type="button"
                      class={['sidebar-nav-item', { active: app().activeNav() === item.key }]}
                      aria-current={app().activeNav() === item.key ? 'page' : undefined}
                      onClick={() => app().setActiveNav(item.key)}
                    >
                      <span class="sidebar-nav-main">
                        <IconComp size={16} />
                        <span>{item.label}</span>
                      </span>
                      <span class="sidebar-nav-end">
                        <Show when={item.alertTone?.()}>
                          {(tone) => (
                            <span
                              class={['status-dot', tone()]}
                              aria-hidden="true"
                            />
                          )}
                        </Show>
                        <Show when={item.badge?.()}>
                          {(badgeText) => <span class="sidebar-nav-count num">{badgeText()}</span>}
                        </Show>
                      </span>
                    </button>
                  )
                }}
              </For>
            </nav>
          </div>

          {/* Persistent Engine & Route Mode Dock */}
          <div class="sidebar-dock" aria-label="运行控制栏">
            <div class="sidebar-dock-row">
              <div class="sidebar-status-group">
                <span class="sidebar-status">
                  <span class={['status-dot', statusTone()]} aria-hidden="true" />
                  <strong>{statusText()}</strong>
                </span>
                <Show when={app().status().ready && app().status().uptime_secs != null}>
                  <span class="sidebar-uptime num">{formatUptime(app().status().uptime_secs)}</span>
                </Show>
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
                    启动
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
                  停止
                </Button>
              </Show>
            </div>

            <div class="seg-rail full" role="group" aria-label="分流模式切换">
              <button
                type="button"
                class={['seg-btn', { active: app().status().route_mode === 'rule' }]}
                aria-pressed={app().status().route_mode === 'rule' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('rule')}
              >
                规则分流
              </button>
              <button
                type="button"
                class={['seg-btn', { active: app().status().route_mode === 'global' }]}
                aria-pressed={app().status().route_mode === 'global' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('global')}
              >
                全局代理
              </button>
            </div>

            <div class="sidebar-dock-foot">
              <div class="sidebar-speed num" aria-label="实时速率">
                <span>↓ {formatSpeed(app().traffic().down ?? 0)}</span>
                <span>↑ {formatSpeed(app().traffic().up ?? 0)}</span>
              </div>
            </div>
          </div>
        </aside>
      </Show>

      {/* Mobile Compact Header + Global Control Strip */}
      <Show when={isMobile()}>
        <header class="mobile-header-stack">
          <div class="mobile-topbar">
            <div class="mobile-topbar-left">
              <LogoIcon size={24} />
              <span class="sidebar-brand-name">Harbor</span>
              <span class="status-inline">
                <span class={['status-dot', statusTone()]} aria-hidden="true" />
                <span>{statusText()}</span>
              </span>
            </div>
            <div class="mobile-topbar-right">
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
                    启动
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
                  停止
                </Button>
              </Show>
              <button
                type="button"
                class="icon-button"
                aria-label="打开命令面板"
                onClick={() => setCmdOpen(true)}
              >
                <SearchIcon size={ICON.md} />
              </button>
            </div>
          </div>

          <div class="mobile-control-bar">
            <div class="seg-rail" role="group" aria-label="分流模式切换">
              <button
                type="button"
                class={['seg-btn', { active: app().status().route_mode === 'rule' }]}
                aria-pressed={app().status().route_mode === 'rule' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('rule')}
              >
                规则分流
              </button>
              <button
                type="button"
                class={['seg-btn', { active: app().status().route_mode === 'global' }]}
                aria-pressed={app().status().route_mode === 'global' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('global')}
              >
                全局代理
              </button>
            </div>
            <div class="sidebar-speed num" aria-label="实时速率">
              <span>↓ {formatSpeed(app().traffic().down ?? 0)}</span>
              <span>↑ {formatSpeed(app().traffic().up ?? 0)}</span>
            </div>
          </div>
        </header>
      </Show>

      {/* Main Single Workspace */}
      <main class="app-main">
        <div class="app-content">
          <Show when={routingAlertCount() > 0 && app().activeNav() !== 'routing'}>
            <div class="circuit-banner danger" role="alert">
              <div class="circuit-banner-main">
                <span class="status-dot crimson pulse" aria-hidden="true" />
                <span>
                  有 {routingAlertCount()} 项设备、独立端口或规则所绑定的节点当前不可用，已自动阻断连接以防意外回退
                </span>
              </div>
              <div class="circuit-banner-actions">
                <Button
                  tone="secondary"
                  size="sm"
                  onClick={() => app().navigateToRouting(firstBlockedRoutingTab())}
                >
                  查看分流隔离
                </Button>
                <Show when={!(app().activeNav() === 'nodes' && app().nodesTab() === 'sources')}>
                  <Button
                    tone="ghost"
                    size="sm"
                    onClick={() => app().navigateToNodes('sources')}
                  >
                    检查节点源
                  </Button>
                </Show>
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

          <Switch>
            <Match when={app().activeNav() === 'nodes'}>
              <NodesView
                app={app()}
                onInspectSub={setInspectingSub}
                onOpenSchedule={() => setShowScheduleModal(true)}
              />
            </Match>
            <Match when={app().activeNav() === 'routing'}>
              <RoutingView app={app()} />
            </Match>
            <Match when={app().activeNav() === 'connections'}>
              <ConnectionsView app={app()} />
            </Match>
            <Match when={app().activeNav() === 'settings'}>
              <SettingsView
                app={app()}
                onOpenSchedule={() => setShowScheduleModal(true)}
              />
            </Match>
          </Switch>
        </div>
      </main>

      {/* Mobile Bottom Navigation (4 Items) */}
      <Show when={isMobile()}>
        <nav class="mobile-dock" role="navigation" aria-label="主导航">
          <For each={navItems}>
            {(item) => {
              const IconComp = item.icon
              return (
                <button
                  type="button"
                  class={['mobile-dock-item', { active: app().activeNav() === item.key }]}
                  aria-current={app().activeNav() === item.key ? 'page' : undefined}
                  onClick={() => app().setActiveNav(item.key)}
                >
                  <IconComp size={16} />
                  <span>{item.shortLabel}</span>
                </button>
              )
            }}
          </For>
        </nav>
      </Show>

      <CommandPalette
        open={cmdOpen()}
        onClose={() => setCmdOpen(false)}
        app={app()}
      />

      <VpsTasks tasks={app().vpsTasks()} />
      <ToastStack toasts={app().toasts()} onDismiss={app().dismissToast} />

      <NodeModal
        open={app().showNodeModal()}
        initialMode={app().nodeModalMode()}
        nodeType={app().nodeType()}
        setNodeType={app().setNodeType}
        form={app().nodeForm()}
        setForm={app().setNodeForm}
        loading={app().vpsDeploying() || ['addNode', 'importNodes'].some((a) => app().pendingActions().has(a))}
        onClose={app().closeNodeModal}
        onSubmit={app().handleAddNode}
        onImport={app().handleImportNodes}
        onDeployVps={app().handleDeployVps}
        vpsSupported={app().status().vps_supported !== false}
        relayCandidates={app().ruleNodeNames()}
      />

      <SubDetailModal
        sub={inspectingSub()}
        onClose={() => setInspectingSub(null)}
        onToggleNode={app().handleSetNodeDisabled}
      />

      <ScheduleModal
        open={showScheduleModal()}
        saving={app().pendingActions().has('scheduledRefresh')}
        onClose={() => setShowScheduleModal(false)}
        onSave={app().handleSaveScheduledRefresh}
      />

      <ConfirmModal
        open={app().confirmState().open}
        title={app().confirmState().title}
        message={app().confirmState().message}
        onCancel={app().closeConfirm}
        onConfirm={() => {
          const fn = app().confirmState().onConfirm
          app().closeConfirm()
          fn?.()
        }}
      />
    </div>
  )
}
