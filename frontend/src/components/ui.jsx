import { LoaderCircle } from 'lucide-react'
import { classNames } from '../utils.js'

export function LogoIcon({ size = 20 }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 64 64"
      xmlns="http://www.w3.org/2000/svg"
      className="brand-icon"
    >
      <circle cx="32" cy="37" r="21" fill="#a78bfa"/>
      <polygon points="13,30 21,8 30,25" fill="#a78bfa"/>
      <polygon points="51,30 43,8 34,25" fill="#a78bfa"/>
      <circle cx="24" cy="35" r="3.5" fill="#fff"/>
      <circle cx="40" cy="35" r="3.5" fill="#fff"/>
      <path d="M30 42 L34 42 L32 45Z" fill="#fff"/>
    </svg>
  )
}

export function Button({
  children,
  tone = 'default',
  size = 'md',
  icon,
  loading,
  className,
  disabled,
  ...props
}) {
  const isDisabled = Boolean(disabled || loading)
  return (
    <button
      type="button"
      className={classNames('btn', `btn-${tone}`, `btn-${size}`, className)}
      {...props}
      disabled={isDisabled}
      aria-busy={loading ? true : undefined}
    >
      {loading ? <LoaderCircle className="spin" size={size === 'sm' ? 12 : 14} /> : icon}
      <span>{children}</span>
    </button>
  )
}

export function SectionCard({ header, children, className, bodyClassName }) {
  return (
    <section className={classNames('panel-card', className)}>
      {header}
      <div className={classNames('panel-card-body', bodyClassName)}>{children}</div>
    </section>
  )
}

export function ToastStack({ toasts }) {
  if (toasts.length === 0) return null
  
  return (
    <div className="toast-stack">
      {toasts.map((toast) => (
        <div key={toast.id} className={classNames('toast', toast.tone)}>
          {toast.tone === 'success' ? (
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M20 6 9 17l-5-5"/>
            </svg>
          ) : toast.tone === 'error' ? (
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10"/>
              <line x1="15" x2="9" y1="9" y2="15"/>
              <line x1="9" x2="15" y1="9" y2="15"/>
            </svg>
          ) : (
            <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10"/>
              <line x1="12" x2="12" y1="16" y2="12"/>
              <line x1="12" x2="12.01" y1="8" y2="8"/>
            </svg>
          )}
          <span>{toast.message}</span>
        </div>
      ))}
    </div>
  )
}
