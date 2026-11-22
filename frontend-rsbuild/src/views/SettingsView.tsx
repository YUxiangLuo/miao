import { Show } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { McpControl } from '../components/McpControl'
import { ConnectivityCheck } from '../components/ConnectivityCheck'
import { classNames, formatUptime } from '../utils'
import type { RouteMode } from '../types/api'
import type { useAppController } from '../hooks/useAppController'
import { ClockIcon, MoonIcon, PlayIcon, SquareIcon, SunIcon } from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export interface SettingsViewProps {
  app: AppController
  theme: 'light' | 'dark'
  onToggleTheme: () => void
  onOpenSchedule: () => void
}

export function SettingsView(props: SettingsViewProps): JSX.Element {
  const app = () => props.app

  const switchRouteMode = (mode: RouteMode) => {
    if (mode === app().status().route_mode) return
    app().handleOpenSetRouteModeConfirm(mode)
  }

  return (
    <>
      <header class="page-header">
        <div>
          <h1 class="page-title">系统设置</h1>
          <p class="page-subtitle">核心服务状态、自动化策略、MCP 接口与版本更新</p>
        </div>
      </header>

      {/* Group 1: Core Service */}
      <section class="surface-box" aria-label="核心服务设置">
        <div class="surface-header">
          <h2 class="surface-title">核心服务</h2>
        </div>
        <div class="hairline-list">
          <div class="hairline-row">
            <div class="row-main">
              <div class="row-title">
                <span
                  class={classNames(
                    'status-dot',
                    app().status().ready ? 'emerald pulse' : app().status().initializing ? 'amber' : 'muted',
                  )}
                  aria-hidden="true"
                />
                <span>sing-box 数据面引擎</span>
                <span class="mono-tag">{app().status().platform || 'linux'}</span>
              </div>
              <div class="row-sub mono">
                {app().status().ready
                  ? `PID ${app().status().pid ?? '--'} · 已运行 ${formatUptime(app().status().uptime_secs)}`
                  : app().status().initializing
                    ? '正在校验配置并启动服务…'
                    : '服务当前已停止'}
              </div>
            </div>
            <div class="row-end">
              <ConnectivityCheck
                status={app().status()}
                backendUnreachable={app().backendUnreachable()}
              />
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
                    启动服务
                  </Button>
                }
              >
                <Button
                  tone="danger"
                  size="sm"
                  icon={<SquareIcon size={ICON.xs} />}
                  loading={app().pendingActions().has('stopService')}
                  onClick={() => app().handleOpenStopServiceConfirm()}
                >
                  停止服务
                </Button>
              </Show>
            </div>
          </div>

          <div class="hairline-row">
            <div class="row-main">
              <div class="row-title">分流模式</div>
              <div class="row-sub">
                规则分流优先让内网与中国大陆直连；全局代理关闭内置大陆直连分流。
              </div>
            </div>
            <div class="row-end">
              <div class="seg-rail" role="group" aria-label="分流模式设置">
                <button
                  type="button"
                  class={classNames('seg-btn', app().status().route_mode === 'rule' && 'active')}
                  aria-pressed={app().status().route_mode === 'rule' ? 'true' : 'false'}
                  disabled={app().pendingActions().has('routeMode')}
                  onClick={() => switchRouteMode('rule')}
                >
                  规则分流
                </button>
                <button
                  type="button"
                  class={classNames('seg-btn', app().status().route_mode === 'global' && 'active')}
                  aria-pressed={app().status().route_mode === 'global' ? 'true' : 'false'}
                  disabled={app().pendingActions().has('routeMode')}
                  onClick={() => switchRouteMode('global')}
                >
                  全局代理
                </button>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Group 2: Automation & MCP */}
      <section class="surface-box" aria-label="自动化与外部集成">
        <div class="surface-header">
          <h2 class="surface-title">自动化与 AI 接入</h2>
        </div>
        <div class="hairline-list">
          <div class="hairline-row">
            <div class="row-main">
              <div class="row-title">订阅定时刷新</div>
              <div class="row-sub">
                按系统本地时区设置每日自动拉取订阅节点并热更新配置的时间点。
              </div>
            </div>
            <div class="row-end">
              <Button
                tone="secondary"
                size="sm"
                icon={<ClockIcon size={ICON.xs} />}
                onClick={props.onOpenSchedule}
              >
                配置定时刷新
              </Button>
            </div>
          </div>

          <div class="hairline-row">
            <div class="row-main">
              <div class="row-title">Model Context Protocol (MCP)</div>
              <div class="row-sub">
                开启后可通过本地 HTTP 端点接入 Claude Code 或 Codex 查询状态、测速与管理配置。
              </div>
            </div>
            <div class="row-end">
              <McpControl
                enabled={app().status().mcp}
                pending={app().pendingActions().has('toggleMcp')}
                onToggle={(enabled) => { void app().handleToggleMcp(enabled) }}
                showToast={app().showToast}
              />
            </div>
          </div>
        </div>
      </section>

      {/* Group 3: Appearance & Version */}
      <section class="surface-box" aria-label="界面与版本信息">
        <div class="surface-header">
          <h2 class="surface-title">界面与版本</h2>
        </div>
        <div class="hairline-list">
          <div class="hairline-row">
            <div class="row-main">
              <div class="row-title">界面外观</div>
              <div class="row-sub">在浅色暖纸与深色碳黑主题之间切换</div>
            </div>
            <div class="row-end">
              <div class="seg-rail" role="group" aria-label="界面外观">
                <button
                  type="button"
                  class={classNames('seg-btn', props.theme === 'light' && 'active')}
                  aria-pressed={props.theme === 'light' ? 'true' : 'false'}
                  onClick={() => { if (props.theme !== 'light') props.onToggleTheme() }}
                >
                  <SunIcon size={13} />
                  <span>浅色</span>
                </button>
                <button
                  type="button"
                  class={classNames('seg-btn', props.theme === 'dark' && 'active')}
                  aria-pressed={props.theme === 'dark' ? 'true' : 'false'}
                  onClick={() => { if (props.theme !== 'dark') props.onToggleTheme() }}
                >
                  <MoonIcon size={13} />
                  <span>深色</span>
                </button>
              </div>
            </div>
          </div>

          <div class="hairline-row">
            <div class="row-main">
              <div class="row-title">
                <span>Harbor 版本</span>
                <span class="mono-tag">v{(app().versionInfo().current || '未知').replace(/^v/, '')}</span>
                <Show when={app().versionInfo().has_update && app().versionInfo().latest}>
                  {(latest) => <span class="mono-tag emerald">新版本 {latest()}</span>}
                </Show>
              </div>
              <div class="row-sub">
                {app().upgrading() && app().upgradeMessage()
                  ? app().upgradeMessage()
                  : app().versionInfo().upgrade_supported === false
                    ? 'Windows 桌面版请前往 GitHub Release 下载最新安装包覆盖安装'
                    : '检查 GitHub Release 最新版本并在线更新'}
              </div>
            </div>
            <div class="row-end">
              <Button
                tone={app().versionInfo().has_update ? 'primary' : 'secondary'}
                size="sm"
                loading={app().upgrading()}
                onClick={() => { void app().handleUpgradeClick() }}
              >
                {app().versionInfo().has_update ? '立即更新' : '检查更新'}
              </Button>
            </div>
          </div>
        </div>
      </section>
    </>
  )
}
