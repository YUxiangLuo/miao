import { Show, createEffect, createSignal, flush } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { ICON } from '../tokens'
import { classNames, formatUptime } from '../utils'
import type { useAppController } from '../hooks/useAppController'
import type { RouteMode } from '../types/api'
import { McpControl } from './McpControl'
import { Button } from './ui'
import { ChevronDownIcon, PlayIcon, SquareIcon } from './icons'

export interface GatewayPopoverProps {
  app: ReturnType<typeof useAppController>
}

export function GatewayPopover(props: GatewayPopoverProps): JSX.Element {
  const app = () => props.app
  const [open, setOpen] = createSignal(false)
  let wrapEl: HTMLDivElement | undefined
  let triggerEl: HTMLButtonElement | undefined

  const statusTone = () => {
    if (app().status().ready) return 'emerald pulse'
    if (app().status().initializing) return 'amber'
    return 'crimson'
  }

  const statusLabel = () => {
    if (app().status().ready) return '运行中'
    if (app().status().initializing) return '启动中'
    return '已停止'
  }

  const closePopover = (restoreFocus = false) => {
    setOpen(false)
    if (restoreFocus) triggerEl?.focus()
  }

  const switchRouteMode = (mode: RouteMode) => {
    if (mode === app().status().route_mode) return
    setOpen(false)
    app().handleOpenSetRouteModeConfirm(mode)
  }

  createEffect(
    () => open(),
    (isOpen) => {
      if (!isOpen) return

      const handlePointerDown = (event: PointerEvent | MouseEvent) => {
        const target = event.target
        if (!(target instanceof Node)) return
        if (wrapEl?.contains(target)) return
        if (target instanceof Element && target.closest('.modal-overlay, .modal-backdrop')) return
        closePopover(false)
        flush()
      }

      const handleKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          if (document.querySelector('.modal-overlay, .modal-backdrop')) return
          event.preventDefault()
          event.stopPropagation()
          closePopover(true)
          flush()
        }
      }

      document.addEventListener('pointerdown', handlePointerDown)
      document.addEventListener('keydown', handleKeyDown)
      return () => {
        document.removeEventListener('pointerdown', handlePointerDown)
        document.removeEventListener('keydown', handleKeyDown)
      }
    },
  )

  return (
    <div ref={(el) => { wrapEl = el }} class="gateway-wrap">
      <button
        ref={(el) => { triggerEl = el }}
        type="button"
        class={classNames('gateway-trigger', open() && 'open')}
        aria-haspopup="dialog"
        aria-expanded={open() ? 'true' : 'false'}
        title="网关引擎控制"
        onClick={() => setOpen((prev) => !prev)}
      >
        <span class={classNames('status-dot', statusTone())} aria-hidden="true" />
        <span>{statusLabel()}</span>
        <ChevronDownIcon size={13} />
      </button>

      <Show when={open()}>
        <div class="gateway-popover" role="dialog" aria-label="网关引擎控制">
          <div class="gateway-section">
            <div class="gateway-row">
              <div class="row-main">
                <div class="row-title">
                  <span>网关引擎</span>
                  <span class="mono-tag">{app().status().platform || 'linux'}</span>
                </div>
                <div class="gateway-meta">
                  PID {app().status().pid ?? '--'} · 在线 {formatUptime(app().status().uptime_secs)}
                </div>
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
                    启动引擎
                  </Button>
                }
              >
                <Button
                  tone="danger"
                  size="sm"
                  icon={<SquareIcon size={ICON.xs} />}
                  loading={app().pendingActions().has('stopService')}
                  onClick={() => {
                    setOpen(false)
                    app().handleOpenStopServiceConfirm()
                  }}
                >
                  停止引擎
                </Button>
              </Show>
            </div>
          </div>

          <div class="gateway-section">
            <div class="gateway-section-label">分流保护模式</div>
            <div class="seg-rail" role="group" aria-label="分流保护模式">
              <button
                type="button"
                class={classNames('seg-btn', app().status().route_mode === 'rule' && 'active')}
                aria-pressed={app().status().route_mode === 'rule' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('rule')}
              >
                大陆办公直连优先
              </button>
              <button
                type="button"
                class={classNames('seg-btn', app().status().route_mode === 'global' && 'active')}
                aria-pressed={app().status().route_mode === 'global' ? 'true' : 'false'}
                disabled={app().pendingActions().has('routeMode')}
                onClick={() => switchRouteMode('global')}
              >
                全量出海排障
              </button>
            </div>
          </div>

          <div class="gateway-section">
            <McpControl
              enabled={app().status().mcp}
              pending={app().pendingActions().has('toggleMcp')}
              onToggle={(enabled) => { void app().handleToggleMcp(enabled) }}
              showToast={app().showToast}
            />
          </div>

          <div class="gateway-section">
            <div class="gateway-row">
              <div class="row-main">
                <div class="row-title">
                  <span class="mono">固件 {app().versionInfo().current || '未知'}</span>
                  <Show when={app().versionInfo().has_update && app().versionInfo().latest}>
                    {(latest) => <span class="mono-tag">可升级 {latest()}</span>}
                  </Show>
                </div>
                <Show when={app().upgrading() && app().upgradeMessage()}>
                  <div class="row-sub">{app().upgradeMessage()}</div>
                </Show>
              </div>
              <Button
                tone={app().versionInfo().has_update ? 'primary' : 'secondary'}
                size="sm"
                loading={app().upgrading()}
                onClick={() => {
                  if (app().versionInfo().has_update && app().versionInfo().latest) {
                    setOpen(false)
                  }
                  void app().handleUpgradeClick()
                }}
              >
                {app().versionInfo().has_update ? '立即升级' : '检查更新'}
              </Button>
            </div>
          </div>
        </div>
      </Show>
    </div>
  )
}
