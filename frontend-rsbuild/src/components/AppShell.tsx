import { Match, Show, Switch, createMemo, createSignal } from 'solid-js'
import { ICON, LOGO_SIZE } from '../tokens'
import { useTheme } from '../hooks/useTheme'
import { Button, LogoIcon, ToastStack } from './ui'
import { ConfirmModal } from './ConfirmModal'
import { NodeModal } from './node-modal/NodeModal'
import { SubDetailModal } from './SubDetailModal'
import { ScheduleModal } from './ScheduleModal'
import { VpsTasks } from './VpsTasks'
import { buildTerminalRows } from '../domain/terminals'
import { buildAccessPortRows } from '../domain/accessPorts'
import { OverviewView } from '../views/OverviewView'
import { TerminalsView } from '../views/TerminalsView'
import { AccessPortsView } from '../views/AccessPortsView'
import { LinesView } from '../views/LinesView'
import { PoliciesView } from '../views/PoliciesView'
import { AuditView } from '../views/AuditView'
import { SystemView } from '../views/SystemView'
import { classNames, formatSpeed } from '../utils'
import type { ConsoleNavKey } from '../hooks/useAppData'
import type { SubStatus } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import {
  ActivityIcon,
  ArrowDownIcon,
  ArrowUpIcon,
  LayoutDashboardIcon,
  MenuIcon,
  MonitorSmartphoneIcon,
  MoonIcon,
  NetworkIcon,
  PlusIcon,
  RouteIcon,
  SettingsIcon,
  StoreIcon,
  SunIcon,
} from './icons'

type AppController = ReturnType<typeof useAppController>

const VIEW_META: Record<ConsoleNavKey, { title: string; subtitle: string }> = {
  overview: {
    title: '网络总览',
    subtitle: '外贸办公网络运行态势、防关联熔断告警与实时跨境业务矩阵',
  },
  terminals: {
    title: '工位与终端',
    subtitle: '局域网电脑与业务手机 DHCP 识别、员工备注及整机出口单事务分配',
  },
  'access-ports': {
    title: '多店铺专属端口',
    subtitle: '单机多指纹浏览器窗口 / 社媒客户端局域网独立代理入口（静态 IP 封存网关内）',
  },
  lines: {
    title: '专线与线路池',
    subtitle: '独享静态住宅 SOCKS5（含前置跳板拓扑）、自建 VPS 专线与公共出海订阅池',
  },
  policies: {
    title: '业务分流策略',
    subtitle: '外贸社媒/电商/收款平台一键分流与大陆办公网银直连保护规则',
  },
  audit: {
    title: '实时流量审计',
    subtitle: '按办公工位、多店铺专属端口与出口通道多维筛选实时连接并执行连通性诊断',
  },
  system: {
    title: '系统与运维',
    subtitle: '转发服务启停、全局分流模式保护、AI 运维 MCP 接口与固件在线升级',
  },
}

export function AppShell(props: { app: AppController }) {
  const app = () => props.app
  const { theme, toggle: toggleTheme } = useTheme()
  const [mobileMenuOpen, setMobileMenuOpen] = createSignal(false)
  const [inspectingSub, setInspectingSub] = createSignal<SubStatus | null>(null)
  const [showScheduleModal, setShowScheduleModal] = createSignal(false)

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

  const failClosedSeatCount = createMemo(() => terminalRows().filter((r) => r.failClosed).length)
  const failClosedPortCount = createMemo(() => accessPortRows().filter((r) => r.blocked).length)

  const selectNav = (key: ConsoleNavKey) => {
    app().setActiveNav(key)
    setMobileMenuOpen(false)
  }

  const currentMeta = createMemo(() => VIEW_META[app().activeNav()])

  return (
    <div class="hg-app">
      <div
        class={classNames('hg-mobile-backdrop', mobileMenuOpen() && 'open')}
        onClick={() => setMobileMenuOpen(false)}
      />

      <aside class={classNames('hg-sidebar', mobileMenuOpen() && 'mobile-open')} aria-label="主导航">
        <div class="hg-brand">
          <LogoIcon size={LOGO_SIZE.topbar} />
          <div class="hg-brand-copy">
            <span class="hg-brand-title">HarborGate</span>
            <span class="hg-brand-sub">海港网关 · 外贸企业网络</span>
          </div>
        </div>

        <nav class="hg-nav">
          <div class="hg-nav-group">
            <div class="hg-nav-group-label">态势感知</div>
            <button
              type="button"
              class={classNames('hg-nav-item', app().activeNav() === 'overview' && 'active')}
              onClick={() => selectNav('overview')}
            >
              <span class="hg-nav-item-main">
                <LayoutDashboardIcon size={ICON.md} />
                <span>网络总览</span>
              </span>
              <Show when={failClosedSeatCount() + failClosedPortCount() > 0}>
                <span class="hg-nav-pill alert">{failClosedSeatCount() + failClosedPortCount()} 告警</span>
              </Show>
            </button>
          </div>

          <div class="hg-nav-group">
            <div class="hg-nav-group-label">终端与多店铺防关联</div>
            <button
              type="button"
              class={classNames('hg-nav-item', app().activeNav() === 'terminals' && 'active')}
              onClick={() => selectNav('terminals')}
            >
              <span class="hg-nav-item-main">
                <MonitorSmartphoneIcon size={ICON.md} />
                <span>工位与终端</span>
              </span>
              <span class={classNames('hg-nav-pill', failClosedSeatCount() > 0 && 'alert')}>
                {terminalRows().length}
              </span>
            </button>

            <button
              type="button"
              class={classNames('hg-nav-item', app().activeNav() === 'access-ports' && 'active')}
              onClick={() => selectNav('access-ports')}
            >
              <span class="hg-nav-item-main">
                <StoreIcon size={ICON.md} />
                <span>多店铺专属端口</span>
              </span>
              <span class={classNames('hg-nav-pill', failClosedPortCount() > 0 && 'alert')}>
                {accessPortRows().length}
              </span>
            </button>
          </div>

          <div class="hg-nav-group">
            <div class="hg-nav-group-label">出口线路与分流</div>
            <button
              type="button"
              class={classNames('hg-nav-item', app().activeNav() === 'lines' && 'active')}
              onClick={() => selectNav('lines')}
            >
              <span class="hg-nav-item-main">
                <NetworkIcon size={ICON.md} />
                <span>专线与线路池</span>
              </span>
              <span class="hg-nav-pill">{app().nodes().length + app().subs().length}</span>
            </button>

            <button
              type="button"
              class={classNames('hg-nav-item', app().activeNav() === 'policies' && 'active')}
              onClick={() => selectNav('policies')}
            >
              <span class="hg-nav-item-main">
                <RouteIcon size={ICON.md} />
                <span>业务分流策略</span>
              </span>
              <span class="hg-nav-pill">{app().rules().length}</span>
            </button>
          </div>

          <div class="hg-nav-group">
            <div class="hg-nav-group-label">审计与系统</div>
            <button
              type="button"
              class={classNames('hg-nav-item', app().activeNav() === 'audit' && 'active')}
              onClick={() => selectNav('audit')}
            >
              <span class="hg-nav-item-main">
                <ActivityIcon size={ICON.md} />
                <span>实时流量审计</span>
              </span>
              <span class="hg-nav-pill">{app().connectionsInfo().connections.length}</span>
            </button>

            <button
              type="button"
              class={classNames('hg-nav-item', app().activeNav() === 'system' && 'active')}
              onClick={() => selectNav('system')}
            >
              <span class="hg-nav-item-main">
                <SettingsIcon size={ICON.md} />
                <span>系统与运维</span>
              </span>
              <Show when={app().versionInfo().has_update}>
                <span class="hg-nav-pill">新版本</span>
              </Show>
            </button>
          </div>
        </nav>

        <div class="hg-sidebar-footer">
          <div class="hg-sidebar-health">
            <span
              class={classNames(
                'hg-health-dot',
                app().status().ready
                  ? 'ready'
                  : app().status().initializing
                    ? 'warn'
                    : 'stopped',
              )}
            >
              {app().status().ready ? '跨境引擎运行中' : app().status().initializing ? '正在初始化…' : '引擎已停止'}
            </span>
          </div>
          <div class="hg-sidebar-meta mono">
            <span>{app().status().platform}</span>
            <span>{app().versionInfo().current ? `v${app().versionInfo().current.replace(/^v/, '')}` : 'HarborGate'}</span>
          </div>
        </div>
      </aside>

      <div class="hg-main-wrap">
        <header class="hg-topbar">
          <div class="hg-topbar-left">
            <button
              type="button"
              class="icon-button hg-mobile-menu-btn"
              aria-label="打开导航菜单"
              onClick={() => setMobileMenuOpen(true)}
            >
              <MenuIcon size={ICON.lg} />
            </button>
            <div class="hg-page-heading">
              <h1>{currentMeta().title}</h1>
              <p>{currentMeta().subtitle}</p>
            </div>
          </div>

          <div class="hg-topbar-right">
            <div class="hg-telemetry-pill num" aria-label="实时总吞吐">
              <span class="hg-telemetry-item down">
                <ArrowDownIcon size={ICON.xs} />
                {formatSpeed(app().traffic().down ?? 0)}
              </span>
              <span style={{ color: 'var(--border-strong)' }}>|</span>
              <span class="hg-telemetry-item up">
                <ArrowUpIcon size={ICON.xs} />
                {formatSpeed(app().traffic().up ?? 0)}
              </span>
            </div>

            <Button
              tone="secondary"
              size="sm"
              icon={<PlusIcon size={ICON.xs} />}
              onClick={() => app().openNodeModal('socks-residential')}
            >
              新增独享专线
            </Button>

            <button
              type="button"
              class="icon-button"
              aria-label={theme() === 'dark' ? '切换为浅色主题' : '切换为深色主题'}
              title={theme() === 'dark' ? '切换为明亮企业工作区' : '切换为深色夜览模式'}
              onClick={toggleTheme}
            >
              {theme() === 'dark' ? <SunIcon size={ICON.md} /> : <MoonIcon size={ICON.md} />}
            </button>
          </div>
        </header>

        <main class="hg-workspace">
          <Switch>
            <Match when={app().activeNav() === 'overview'}>
              <OverviewView app={app()} />
            </Match>
            <Match when={app().activeNav() === 'terminals'}>
              <TerminalsView app={app()} />
            </Match>
            <Match when={app().activeNav() === 'access-ports'}>
              <AccessPortsView app={app()} />
            </Match>
            <Match when={app().activeNav() === 'lines'}>
              <LinesView
                app={app()}
                onInspectSub={(sub) => setInspectingSub(sub)}
                onOpenSchedule={() => setShowScheduleModal(true)}
              />
            </Match>
            <Match when={app().activeNav() === 'policies'}>
              <PoliciesView app={app()} />
            </Match>
            <Match when={app().activeNav() === 'audit'}>
              <AuditView app={app()} />
            </Match>
            <Match when={app().activeNav() === 'system'}>
              <SystemView app={app()} />
            </Match>
          </Switch>
        </main>
      </div>

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
