import { Show, createMemo } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { McpControl } from '../components/McpControl'
import { formatUptime } from '../utils'
import type { useAppController } from '../hooks/useAppController'
import { ClockIcon, MoonIcon, SunIcon } from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export interface SettingsViewProps {
  app: AppController
  theme: 'light' | 'dark'
  onToggleTheme: () => void
  onOpenSchedule: () => void
}

export function SettingsView(props: SettingsViewProps): JSX.Element {
  const app = () => props.app

  const runtimeSummary = createMemo(() => {
    const status = app().status()
    if (!status.ready) return ''
    const parts: string[] = []
    if (status.pid != null) parts.push(`内核 PID ${status.pid}`)
    parts.push(`已运行 ${formatUptime(status.uptime_secs)}`)
    return parts.join(' · ')
  })

  return (
    <>
      <header class="page-header">
        <div>
          <h1 class="page-title">系统设置</h1>
          <p class="page-subtitle">自动化刷新策略、AI 接入协议（MCP）、界面外观与版本更新</p>
        </div>
      </header>

      {/* Group 1: Automation & MCP */}
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

      {/* Group 2: Appearance & Version */}
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
                  class={['seg-btn', { active: props.theme === 'light' }]}
                  aria-pressed={props.theme === 'light' ? 'true' : 'false'}
                  onClick={() => { if (props.theme !== 'light') props.onToggleTheme() }}
                >
                  <SunIcon size={13} />
                  <span>浅色</span>
                </button>
                <button
                  type="button"
                  class={['seg-btn', { active: props.theme === 'dark' }]}
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
                <Show when={app().status().platform}>
                  {(plat) => <span class="mono-tag">{plat()}</span>}
                </Show>
                <Show when={app().versionInfo().has_update && app().versionInfo().latest}>
                  {(latest) => <span class="mono-tag emerald">新版本 {latest()}</span>}
                </Show>
              </div>
              <div class="row-sub">
                {app().upgrading() && app().upgradeMessage()
                  ? app().upgradeMessage()
                  : app().versionInfo().upgrade_supported === false
                    ? '点击前往 GitHub Release 下载最新安装包（覆盖安装前请先从托盘退出）'
                    : '检查 GitHub Release 最新版本并在线更新'}
                <Show when={!app().upgrading() && runtimeSummary()}>
                  {(meta) => <span class="mono"> · {meta()}</span>}
                </Show>
              </div>
            </div>
            <div class="row-end">
              <Button
                tone={app().versionInfo().has_update ? 'primary' : 'secondary'}
                size="sm"
                loading={app().upgrading() || app().checkingVersion()}
                onClick={() => { void app().handleUpgradeClick() }}
              >
                {app().versionInfo().has_update
                  ? (app().versionInfo().upgrade_supported === false ? '前往下载' : '立即更新')
                  : '检查更新'}
              </Button>
            </div>
          </div>
        </div>
      </section>
    </>
  )
}
