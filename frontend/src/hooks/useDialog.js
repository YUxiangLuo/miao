import { useEffect, useRef } from 'react'

const FOCUSABLE_SELECTOR = [
  'button:not([disabled])',
  'input:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[href]',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

export function useDialog(open, onClose) {
  const dialogRef = useRef(null)
  const onCloseRef = useRef(onClose)

  useEffect(() => {
    onCloseRef.current = onClose
  }, [onClose])

  useEffect(() => {
    if (!open) return undefined

    const previousFocus = document.activeElement
    const dialog = dialogRef.current
    const focusable = () => [...(dialog?.querySelectorAll(FOCUSABLE_SELECTOR) || [])]
    // 优先聚焦对话框内显式声明 data-autofocus 的控件，
    // 避免焦点默认落在标题栏的关闭按钮上（危险确认场景 Enter 会变成“取消”）
    const preferred = dialog?.querySelector('[data-autofocus]:not([disabled])')
    const firstControl = focusable()[0]
    ;(preferred || firstControl || dialog)?.focus()

    const handleKeyDown = (event) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        onCloseRef.current?.()
        return
      }

      if (event.key !== 'Tab') return
      const controls = focusable()
      if (controls.length === 0) {
        event.preventDefault()
        dialog?.focus()
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
  }, [open])

  return dialogRef
}
