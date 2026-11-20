import { For, Match, Show, Switch, createEffect, createMemo, createSignal } from 'solid-js'
import { ICON } from '../tokens'
import { useTheme } from '../hooks/useTheme'
import { LogoIcon, ToastStack } from './ui'
import { ConfirmModal } from './ConfirmModal'
import { NodeModal } from './node-modal/NodeModal'
import { SubDetailModal } from './SubDetailModal'
import { ScheduleModal } from './ScheduleModal'
import { VpsTasks } from './VpsTasks'
import { GatewayPopover } from './GatewayPopover'
import { CommandPalette, useCommandShortcut } from './CommandPalette'
import { buildTerminalRows } from '../domain/terminals'
import { buildAccessPortRows } from '../domain/accessPorts'
import { StudioView } from '../views/StudioView'
import { EndpointsView } from '../views/EndpointsView'
import { RoutesView } from '../views/RoutesView'
import { PulseView } from '../views/PulseView'
import { classNames, formatSpeed } from '../utils'
import type { ConsoleNavKey } from '../hooks/useAppData'
import type { SubStatus } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import { MoonIcon, SearchIcon, SunIcon } from './icons'

type AppController = ReturnType<typeof useAppController>

function detectMobileViewport(): boolean {
  if (typeof window === 'undefined') return false
  if (window.innerWidth > 0 && window.innerWidth <= 760) return true
  return Boolean(window.matchMedia?.('(max-width: 760px)')?.matches)
}

export function AppShell(props: { app: AppController }) {
  const app = () => props.app
  const { theme, toggle: toggleTheme } = useTheme()
  const [cmdOpen, setCmdOpen] = createSignal(false)
  const [inspectingSub, setInspectingSub] = createSignal<SubStatus | null>(null)
  const [showScheduleModal, setShowScheduleModal] = createSignal(false)
  const [isMobile, setIsMobile] = createSignal(detectMobileViewport())

  useCommandShortcut(() => setCmdOpen((prev) => !prev))

  createEffect(
    () => true,
    () => {
      if (typeof window === 'undefined') return
      const syncViewport = () => setIsMobile(detectMobileViewport())
      window.addEventListener('resize', syncViewport)
      const mql = window.matchMedia?.('(max-width: 760px)')
      mql?.addEventListener?.('change', syncViewport)
      return () => {
        window.removeEventListener('resize', syncViewport)
        mql?.removeEventListener?.('change', syncViewport)
      }
    },
  )

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

  const failClosedCount = createMemo(() => (
    terminalRows().filter((r) => r.failClosed).length
    + accessPortRows().filter((r) => r.blocked).length
  ))

  const navItems: ReadonlyArray<{
    key: ConsoleNavKey
    label: string
    hasAlert: () => boolean
  }> = [
    { key: 'studio', label: '工作台', hasAlert: () => failClosedCount() > 0 },
    { key: 'endpoints', label: '终端与店铺', hasAlert: () => failClosedCount() > 0 },
    { key: 'routes', label: '全球线路', hasAlert: () => false },
    { key: 'pulse', label: '分流与脉冲', hasAlert: () => false },
  ]

  return (
    <div class="studio-shell">
      <header class="studio-topbar">
        <div class="studio-topbar-inner">
          <div class="studio-brand">
            <LogoIcon size={26} />
            <span class="studio-brand-name">Harbor</span>
            <GatewayPopover app={app()} />
          </div>

          <Show when={!isMobile()}>
            <nav class="studio-nav-rail" role="navigation" aria-label="主工作区">
              <For each={navItems}>
                {(item) => (
                  <button
                    type="button"
                    class={classNames('studio-nav-tab', app().activeNav() === item.key && 'active')}
                    aria-current={app().activeNav() === item.key ? 'page' : undefined}
                    onClick={() => app().setActiveNav(item.key)}
                  >
                    <span>{item.label}</span>
                    <Show when={item.hasAlert()}>
                      <span class="studio-nav-dot" aria-hidden="true" />
                    </Show>
                  </button>
                )}
              </For>
            </nav>
          </Show>

          <div class="studio-topbar-right">
            <div class="studio-speed-readout num" aria-label="实时总吞吐">
              <span>↓ {formatSpeed(app().traffic().down ?? 0)}</span>
              <span style={{ color: 'var(--border-strong)' }}>·</span>
              <span>↑ {formatSpeed(app().traffic().up ?? 0)}</span>
            </div>

            <button
              type="button"
              class="studio-cmd-trigger"
              aria-label="打开全局命令面板"
              title="全局命令面板 (⌘K)"
              onClick={() => setCmdOpen(true)}
            >
              <SearchIcon size={13} />
              <span>命令</span>
              <span class="kbd-chip">⌘K</span>
            </button>

            <button
              type="button"
              class="icon-button"
              aria-label={theme() === 'dark' ? '切换为浅色外观' : '切换为深色外观'}
              title={theme() === 'dark' ? '切换为浅色外观' : '切换为深色外观'}
              onClick={toggleTheme}
            >
              {theme() === 'dark' ? <SunIcon size={ICON.md} /> : <MoonIcon size={ICON.md} />}
            </button>
          </div>
        </div>
      </header>

      <main class="studio-canvas">
        <Switch>
          <Match when={app().activeNav() === 'studio'}>
            <StudioView app={app()} />
          </Match>
          <Match when={app().activeNav() === 'endpoints'}>
            <EndpointsView app={app()} />
          </Match>
          <Match when={app().activeNav() === 'routes'}>
            <RoutesView
              app={app()}
              onInspectSub={setInspectingSub}
              onOpenSchedule={() => setShowScheduleModal(true)}
            />
          </Match>
          <Match when={app().activeNav() === 'pulse'}>
            <PulseView app={app()} />
          </Match>
        </Switch>
      </main>

      <Show when={isMobile()}>
        <nav class="studio-mobile-dock" role="navigation" aria-label="主工作区">
          <For each={navItems}>
            {(item) => (
              <button
                type="button"
                class={classNames('studio-nav-tab', app().activeNav() === item.key && 'active')}
                aria-current={app().activeNav() === item.key ? 'page' : undefined}
                onClick={() => app().setActiveNav(item.key)}
              >
                <span>{item.label}</span>
                <Show when={item.hasAlert()}>
                  <span class="studio-nav-dot" aria-hidden="true" />
                </Show>
              </button>
            )}
          </For>
        </nav>
      </Show>

      <CommandPalette
        open={cmdOpen()}
        onClose={() => setCmdOpen(false)}
        app={app()}
        onToggleTheme={toggleTheme}
        theme={theme()}
      />

      <VpsTasks tasks={app().vpsTasks()} />
      <ToastStack toasts={app().toasts()} onDismiss={app().dismissToast} />

      <NodeModal
        open={app().showNodeModal()}
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
