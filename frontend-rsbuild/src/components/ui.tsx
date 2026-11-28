import { For, Show, createUniqueId, omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { ICON } from '../tokens'
import type { Toast } from '../hooks/useApi'
import { CheckIcon, CircleAlertIcon, CircleXIcon, LoaderCircleIcon, XIcon } from './icons'

/**
 * Harbor · 工业精密网络徽标：
 * 深灰墨圆角底座 + 纯白信号桥架构 + 工业蓝/绿在线信标点。
 */
export function LogoIcon(props: { size?: number; class?: string }) {
  const size = () => props.size ?? 28
  return (
    <svg
      width={size()}
      height={size()}
      viewBox="0 0 512 512"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      class={['brand-icon', props.class]}
      aria-hidden="true"
    >
      <rect class="logo-frame" width="512" height="512" rx="116" fill="#005CE6" />
      <rect x="18" y="18" width="476" height="476" rx="98" stroke="#FFFFFF" stroke-opacity="0.18" stroke-width="6" />
      <rect x="132" y="128" width="52" height="256" rx="14" fill="#FFFFFF" />
      <rect x="328" y="128" width="52" height="256" rx="14" fill="#FFFFFF" />
      <rect x="176" y="234" width="160" height="44" rx="8" fill="#FFFFFF" />
      <circle cx="354" cy="154" r="28" fill="#005CE6" />
      <circle cx="354" cy="154" r="16" fill="#4ADE80" />
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
      data-motion="tactile"
      class={['btn', `btn-${props.tone}`, `btn-${size()}`, props.class]}
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

export type ProgressBarTone = 'primary' | 'success' | 'warning' | 'destructive'

export interface ProgressBarProps {
  value: number
  min?: number
  max?: number
  tone?: ProgressBarTone
  label?: string
  valueText?: string
  indeterminate?: boolean
  class?: string
}

export function ProgressBar(props: ProgressBarProps) {
  const labelId = createUniqueId()
  const min = () => (props.min !== undefined && Number.isFinite(props.min) ? props.min : 0)
  const max = () => {
    const candidate = props.max !== undefined && Number.isFinite(props.max) ? props.max : 100
    return candidate > min() ? candidate : min() + 100
  }
  const clamped = () => {
    const raw = Number.isFinite(props.value) ? props.value : min()
    return Math.min(max(), Math.max(min(), raw))
  }
  const percent = () => Math.round(((clamped() - min()) / (max() - min())) * 100)
  const tone = (): ProgressBarTone => props.tone ?? 'primary'

  return (
    <div class={['progress-bar-root', props.class]}>
      <Show when={props.label}>
        {(lbl) => <span id={labelId} class="sr-only">{lbl()}</span>}
      </Show>
      <div
        role="progressbar"
        aria-labelledby={props.label ? labelId : undefined}
        aria-label={!props.label ? (props.valueText ?? '进度') : undefined}
        aria-valuemin={min()}
        aria-valuemax={max()}
        aria-valuenow={props.indeterminate ? undefined : clamped()}
        aria-valuetext={props.valueText}
        class="progress-bar-track"
      >
        <div
          class={[
            'progress-bar-fill',
            tone() !== 'primary' ? tone() : '',
            { 'animate-indeterminate': Boolean(props.indeterminate) },
          ]}
          style={props.indeterminate ? { width: '50%' } : { width: `${percent()}%` }}
        />
      </div>
    </div>
  )
}

export function SectionCard(props: {
  header?: JSX.Element
  children?: JSX.Element
  class?: string
  bodyClass?: string
}) {
  return (
    <section class={['panel-card', props.class]}>
      {props.header}
      <div class={['panel-card-body', props.bodyClass]}>{props.children}</div>
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
            <div
              class={['toast', toast().tone]}
              data-motion="stack-item"
              data-motion-state="open"
            >
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
