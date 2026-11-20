import { For, Show, omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { classNames } from '../utils'
import { ICON } from '../tokens'
import type { Toast } from '../hooks/useApi'
import { CheckIcon, CircleAlertIcon, CircleXIcon, LoaderCircleIcon, XIcon } from './icons'

/**
 * HarborGate · 海港网关 几何港门徽标（无吉祥物）：
 * 深海岩蓝港湾底座 + 双侧航道柱门 H 结构 + 领航青蓝横梁与防关联绿航标。
 */
export function LogoIcon(props: { size?: number; class?: string }) {
  const size = () => props.size ?? 30
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 64 64"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      class={classNames('brand-icon', props.class)}
      aria-hidden="true"
    >
      <rect class="logo-frame" x="3" y="3" width="58" height="58" rx="14" fill="#0F172A" stroke="#1E293B" stroke-width="2" />
      <path d="M16 15H23.5V49H16V15Z" fill="#F8FAFC" />
      <path d="M40.5 15H48V49H40.5V15Z" fill="#F8FAFC" />
      <path d="M23.5 28.5H40.5V35H23.5V28.5Z" fill="#38BDF8" />
      <circle cx="32" cy="19.5" r="3" fill="#0EA5E9" />
      <path d="M27.5 42.5H36.5L32 48.5L27.5 42.5Z" fill="#10B981" />
    </svg>
  )
}

export type ButtonTone = 'primary' | 'secondary' | 'danger' | 'ghost'
export type ButtonSize = 'md' | 'sm'

export interface ButtonProps extends Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, 'class'> {
  tone: ButtonTone
  size?: ButtonSize
  icon?: JSX.Element
  loading?: boolean
  class?: string
}

export function Button(props: ButtonProps) {
  const rest = omit(props, 'children', 'tone', 'size', 'icon', 'loading', 'class', 'disabled', 'type')
  const size = () => props.size ?? 'md'
  const isDisabled = () => Boolean(props.disabled || props.loading)

  return (
    <button
      type={props.type ?? 'button'}
      class={classNames('btn', `btn-${props.tone}`, `btn-${size()}`, props.class)}
      {...rest}
      disabled={isDisabled()}
      aria-busy={props.loading ? 'true' : undefined}
    >
      <Show
        when={props.loading}
        fallback={props.icon}
      >
        <LoaderCircleIcon class="spin" size={size() === 'sm' ? ICON.xs : ICON.sm} />
      </Show>
      <span>{props.children}</span>
    </button>
  )
}

export function SectionCard(props: {
  header?: JSX.Element
  children?: JSX.Element
  class?: string
  bodyClass?: string
}) {
  return (
    <section class={classNames('panel-card', props.class)}>
      {props.header}
      <div class={classNames('panel-card-body', props.bodyClass)}>{props.children}</div>
    </section>
  )
}

export function ToastStack(props: {
  toasts: Toast[]
  onDismiss?: (id: number) => void
}) {
  return (
    <Show when={props.toasts.length > 0}>
      <div class="toast-stack" aria-live="polite" aria-relevant="additions">
        <For each={props.toasts} keyed={(toast) => toast.id}>
          {(toast) => (
            <div class={classNames('toast', toast().tone)}>
              <span class="toast-icon" aria-hidden="true">
                {toast().tone === 'success'
                  ? <CheckIcon size={14} />
                  : toast().tone === 'error'
                    ? <CircleXIcon size={14} />
                    : <CircleAlertIcon size={14} />}
              </span>
              <span class="toast-text">{toast().message}</span>
              <Show when={props.onDismiss}>
                {(dismiss) => (
                  <button
                    type="button"
                    class="toast-close"
                    onClick={() => dismiss()(toast().id)}
                    aria-label="关闭提示"
                  >
                    <XIcon size={12} />
                  </button>
                )}
              </Show>
            </div>
          )}
        </For>
      </div>
    </Show>
  )
}
