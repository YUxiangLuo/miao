import { useCallback, useRef, useState } from 'react'
import { API_HEADERS } from '../utils.js'

const TOAST_DURATION = 3500

export function useToast() {
  const [toasts, setToasts] = useState([])
  const toastIdRef = useRef(0)
  const toastsRef = useRef([])
  const timersRef = useRef(new Map())

  const dismissToast = useCallback((id) => {
    const timer = timersRef.current.get(id)
    if (timer) {
      window.clearTimeout(timer)
      timersRef.current.delete(id)
    }
    toastsRef.current = toastsRef.current.filter((item) => item.id !== id)
    setToasts(toastsRef.current)
  }, [])

  const showToast = useCallback((message, tone = 'info') => {
    // 相同内容且仍在显示的 toast：刷新自动消失时间，不重复堆叠
    const existing = toastsRef.current.find((item) => item.message === message && item.tone === tone)
    if (existing) {
      const oldTimer = timersRef.current.get(existing.id)
      if (oldTimer) window.clearTimeout(oldTimer)
      timersRef.current.set(existing.id, window.setTimeout(() => dismissToast(existing.id), TOAST_DURATION))
      return existing.id
    }

    const id = ++toastIdRef.current
    toastsRef.current = [...toastsRef.current, { id, message, tone }]
    setToasts(toastsRef.current)
    timersRef.current.set(id, window.setTimeout(() => dismissToast(id), TOAST_DURATION))
    return id
  }, [dismissToast])

  return { toasts, showToast, dismissToast }
}

export function useApi({ setLoadingAction }) {
  const apiCall = useCallback(async (endpoint, options = {}, action = '') => {
    setLoadingAction(action)
    try {
      const response = await fetch(`/api/${endpoint}`, { headers: API_HEADERS, ...options })
      const payload = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } finally {
      setLoadingAction('')
    }
  }, [setLoadingAction])

  return { apiCall }
}
