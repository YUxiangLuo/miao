import { Show } from 'solid-js'
import { ICON } from '../tokens'
import { Button } from '../components/ui'
import { McpControl } from '../components/McpControl'
import { classNames, formatUptime } from '../utils'
import type { useAppController } from '../hooks/useAppController'
import {
  PlayIcon,
  RotateCwIcon,
  ServerIcon,
  SettingsIcon,
  ShieldCheckIcon,
  SquareIcon,
} from '../components/icons'

type AppController = ReturnType<typeof useAppController>

export function SystemView(props: { app: AppController }) {
  const app = () => props.app

  return (
    <div class="hg-system-grid">
      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <ServerIcon size={ICON.md} class="icon-accent" />
              网关转发引擎控制
            </h2>
            <p>控制底层定制内核（GoTUN 透明网关 + 多店铺 mixed 代理端口）的运行状态</p>
          </div>
          <span class={classNames('badge', app().status().ready ? 'emerald' : 'danger')}>
            {app().status().ready ? '运行正常' : app().status().initializing ? '正在启动' : '已停止'}
          </span>
        </div>
        <div class="hg-card-body" style={{ display: 'flex', 'flex-direction': 'column', gap: '14px' }}>
          <div class="list-row">
            <div class="list-row-content">
              <div class="list-row-title">运行平台与进程信息</div>
              <div class="list-row-meta mono">
                平台: {app().status().platform} · PID: {app().status().pid ?? '--'} · 在线时长: {formatUptime(app().status().uptime_secs)}
              </div>
            </div>
            <div class="hg-inline-actions">
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
                    启动网关转发
                  </Button>
                }
              >
                <Button
                  tone="danger"
                  size="sm"
                  icon={<SquareIcon size={ICON.xs} />}
                  loading={app().pendingActions().has('stopService')}
                  onClick={app().handleOpenStopServiceConfirm}
                >
                  停止网关转发
                </Button>
              </Show>
            </div>
          </div>
        </div>
      </section>

      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <ShieldCheckIcon size={ICON.md} class="icon-accent" />
              企业整机分流模式保护
            </h2>
            <p>控制透明网关入口对大陆办公与银行流量的默认保护行为（不影响多店铺专属端口）</p>
          </div>
        </div>
        <div class="hg-card-body" style={{ display: 'flex', 'flex-direction': 'column', gap: '12px' }}>
          <div class="hg-segmented" role="group" aria-label="全局分流模式">
            <button
              type="button"
              class={classNames('hg-segment', app().status().route_mode === 'rule' && 'active')}
              aria-pressed={app().status().route_mode === 'rule' ? 'true' : 'false'}
              disabled={app().pendingActions().has('routeMode')}
              onClick={() => app().handleOpenSetRouteModeConfirm('rule')}
            >
              智能企业分流（推荐 · 大陆办公直连）
            </button>
            <button
              type="button"
              class={classNames('hg-segment', app().status().route_mode === 'global' && 'active')}
              aria-pressed={app().status().route_mode === 'global' ? 'true' : 'false'}
              disabled={app().pendingActions().has('routeMode')}
              onClick={() => app().handleOpenSetRouteModeConfirm('global')}
            >
              全量出海排障模式（Global）
            </button>
          </div>
          <p class="drawer-note">
            日常办公请始终保持「智能企业分流」，确保企业微信、飞书、ERP 与对公网银直连不经过代理。仅在排查特殊海外解析问题时临时切换全量模式。
          </p>
        </div>
      </section>

      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <SettingsIcon size={ICON.md} class="icon-accent" />
              AI 自动化运维接口 (MCP)
            </h2>
            <p>允许授权的 AI 运维助手（Claude Code / Codex）通过 Streamable HTTP 协议协助巡检线路与排障</p>
          </div>
        </div>
        <div class="hg-card-body">
          <McpControl
            enabled={app().status().mcp}
            pending={app().pendingActions().has('toggleMcp')}
            onToggle={(enabled) => { void app().handleToggleMcp(enabled) }}
            showToast={app().showToast}
          />
        </div>
      </section>

      <section class="hg-card">
        <div class="hg-card-header">
          <div class="hg-card-title-group">
            <h2>
              <RotateCwIcon size={ICON.md} class="icon-accent" />
              网关固件版本与在线升级
            </h2>
            <p>单二进制原子校验升级，自动保留全部工位档案、专属端口与专线配置</p>
          </div>
        </div>
        <div class="hg-card-body" style={{ display: 'flex', 'flex-direction': 'column', gap: '12px' }}>
          <div class="list-row">
            <div class="list-row-content">
              <div class="list-row-title mono">
                当前版本: {app().versionInfo().current || '未知'}
                <Show when={app().versionInfo().has_update && app().versionInfo().latest}>
                  {(latest) => <span class="badge ocean" style={{ 'margin-left': '8px' }}>可升级至 {latest()}</span>}
                </Show>
              </div>
              <div class="list-row-meta">
                {app().upgrading()
                  ? app().upgradeMessage()
                  : '点击右侧按钮检查最新网关发行版或执行一键升级'}
              </div>
            </div>
            <Button
              tone={app().versionInfo().has_update ? 'primary' : 'secondary'}
              size="sm"
              loading={app().upgrading()}
              onClick={() => { void app().handleUpgradeClick() }}
            >
              {app().versionInfo().has_update ? '立即升级' : '检查更新'}
            </Button>
          </div>
        </div>
      </section>
    </div>
  )
}
