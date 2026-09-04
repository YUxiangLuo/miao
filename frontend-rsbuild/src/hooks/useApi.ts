import { useCallback, useRef, useState } from 'react'
import { API_HEADERS } from '../utils'
import type { ApiResponse } from '../types/api'

const TOAST_DURATION = 3500

export type ToastTone = 'info' | 'success' | 'error'

export interface Toast {
  id: number
  message: string
  tone: ToastTone
}

export function useToast() {
  const [toasts, setToasts] = useState<Toast[]>([])
  const toastIdRef = useRef(0)
  const toastsRef = useRef<Toast[]>([])
  const timersRef = useRef(new Map<number, number>())

  const dismissToast = useCallback((id: number) => {
    const timer = timersRef.current.get(id)
    if (timer) {
      window.clearTimeout(timer)
      timersRef.current.delete(id)
    }
    toastsRef.current = toastsRef.current.filter((item) => item.id !== id)
    setToasts(toastsRef.current)
  }, [])

  const showToast = useCallback((message: string, tone: ToastTone = 'info') => {
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

export function useApi() {
  const [pendingActions, setPendingActions] = useState<ReadonlySet<string>>(new Set())
  const pendingCountsRef = useRef(new Map<string, number>())

  const setActionPending = useCallback((action: string, pending: boolean) => {
    if (!action) return
    const counts = pendingCountsRef.current
    const nextCount = (counts.get(action) ?? 0) + (pending ? 1 : -1)
    if (nextCount > 0) counts.set(action, nextCount)
    else counts.delete(action)
    setPendingActions(new Set(counts.keys()))
  }, [])

  const apiCall = useCallback(async <T = unknown>(endpoint: string, options: RequestInit = {}, action = ''): Promise<ApiResponse<T>> => {
    setActionPending(action, true)
    try {
      const response = await fetch(`/api/${endpoint}`, { headers: API_HEADERS, ...options })
      const payload: ApiResponse<T> = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } finally {
      setActionPending(action, false)
    }
  }, [setActionPending])

  return { apiCall, pendingActions }
}
