import { useCallback, useEffect, useRef, useState } from 'react'
import { API_HEADERS } from '../utils'
import type { ApiResponse } from '../types/api'
import { fetchJson, RequestInterruptedError, WRITE_TIMEOUT_MS } from './request'

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

  useEffect(() => {
    const timers = timersRef.current
    return () => {
      timers.forEach(timer => window.clearTimeout(timer))
      timers.clear()
    }
  }, [])

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
      if (tone !== 'error') timersRef.current.set(existing.id, window.setTimeout(() => dismissToast(existing.id), TOAST_DURATION))
      return existing.id
    }

    const id = ++toastIdRef.current
    toastsRef.current = [...toastsRef.current, { id, message, tone }]
    setToasts(toastsRef.current)
    if (tone !== 'error') timersRef.current.set(id, window.setTimeout(() => dismissToast(id), TOAST_DURATION))
    return id
  }, [dismissToast])

  return { toasts, showToast, dismissToast }
}

export function useApi(onUncertain?: () => void) {
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
      // VPS deployment can include package installation and two SSH operations.
      const timeout = endpoint === 'vps/deploy' ? 600_000 : WRITE_TIMEOUT_MS
      const payload = await fetchJson<ApiResponse<T>>(`/api/${endpoint}`, { headers: API_HEADERS, ...options }, timeout)
      if (!payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } catch (error) {
      if (error instanceof RequestInterruptedError || error instanceof TypeError) {
        onUncertain?.()
        throw new RequestInterruptedError('请求中断或网络异常，操作结果未知；请刷新状态确认，勿直接重复操作')
      }
      throw error
    } finally {
      setActionPending(action, false)
    }
  }, [setActionPending, onUncertain])

  return { apiCall, pendingActions }
}
