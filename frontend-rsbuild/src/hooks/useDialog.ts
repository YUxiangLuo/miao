import { createEffect, createMemo, createSignal, type Accessor } from 'solid-js'

const FOCUSABLE_SELECTOR = [
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[href]',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

export function useDialog<T extends HTMLElement = HTMLDivElement>(
  open: Accessor<boolean>,
  onClose: () => void,
) {
  const [dialogEl, setDialogEl] = createSignal<T | undefined>()
  const isOpen = createMemo(() => Boolean(open()))

  createEffect(
    () => ({ isOpen: isOpen(), dialog: dialogEl() }),
    ({ isOpen: openNow, dialog }) => {
      if (!openNow || !dialog || !dialog.isConnected) return

      const previousFocus = document.activeElement
      const focusable = () => [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)]
      const preferred = dialog.querySelector<HTMLElement>('[data-autofocus]:not([disabled])')
      const firstControl = focusable()[0]
      ;(preferred || firstControl || dialog).focus()

      const handleKeyDown = (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          event.preventDefault()
          onClose()
          return
        }

        if (event.key !== 'Tab') return
        const controls = focusable()
        if (controls.length === 0) {
          event.preventDefault()
          dialog.focus()
          return
        }

        const first = controls[0]
        const last = controls[controls.length - 1]
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault()
          last.focus()
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault()
          first.focus()
        }
      }

      document.addEventListener('keydown', handleKeyDown)
      return () => {
        document.removeEventListener('keydown', handleKeyDown)
        if (previousFocus instanceof HTMLElement) previousFocus.focus()
      }
    },
  )

  return (el: T) => setDialogEl(() => el)
}
