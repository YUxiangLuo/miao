import { createSignal, onCleanup } from 'solid-js'
import { API_HEADERS } from '../utils'
import type { ApiResponse } from '../types/api'
import { fetchJson, isUncertainRequestError, RequestInterruptedError, WRITE_TIMEOUT_MS } from './request'

const TOAST_DURATION = 3500

export type ToastTone = 'info' | 'success' | 'error'

export interface Toast {
  id: number
  message: string
  tone: ToastTone
}

export function useToast() {
  const [toasts, setToasts] = createSignal<Toast[]>([])
  let toastId = 0
  let currentToasts: Toast[] = []
  const timers = new Map<number, number>()

  onCleanup(() => {
    timers.forEach((timer) => window.clearTimeout(timer))
    timers.clear()
  })

  const dismissToast = (id: number) => {
    const timer = timers.get(id)
    if (timer !== undefined) {
      window.clearTimeout(timer)
      timers.delete(id)
    }
    currentToasts = currentToasts.filter((item) => item.id !== id)
    setToasts(currentToasts)
  }

  const showToast = (message: string, tone: ToastTone = 'info') => {
    const existing = currentToasts.find((item) => item.message === message && item.tone === tone)
    if (existing) {
      const oldTimer = timers.get(existing.id)
      if (oldTimer !== undefined) window.clearTimeout(oldTimer)
      if (tone !== 'error') {
        timers.set(existing.id, window.setTimeout(() => dismissToast(existing.id), TOAST_DURATION))
      }
      return existing.id
    }

    const id = ++toastId
    currentToasts = [...currentToasts, { id, message, tone }]
    setToasts(currentToasts)
    if (tone !== 'error') {
      timers.set(id, window.setTimeout(() => dismissToast(id), TOAST_DURATION))
    }
    return id
  }

  return { toasts, showToast, dismissToast }
}

export function useApi(onUncertain?: () => void) {
  const [pendingActions, setPendingActions] = createSignal<ReadonlySet<string>>(new Set())
  const pendingCounts = new Map<string, number>()

  const setActionPending = (action: string, pending: boolean) => {
    if (!action) return
    const nextCount = (pendingCounts.get(action) ?? 0) + (pending ? 1 : -1)
    if (nextCount > 0) pendingCounts.set(action, nextCount)
    else pendingCounts.delete(action)
    setPendingActions(new Set(pendingCounts.keys()))
  }

  const apiCall = async <T = unknown>(
    endpoint: string,
    options: RequestInit = {},
    action = '',
  ): Promise<ApiResponse<T>> => {
    setActionPending(action, true)
    try {
      const timeout = endpoint === 'vps/deploy' ? 600_000 : WRITE_TIMEOUT_MS
      const payload = await fetchJson<ApiResponse<T>>(
        `/api/${endpoint}`,
        { headers: API_HEADERS, ...options },
        timeout,
      )
      if (!payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } catch (error) {
      if (isUncertainRequestError(error)) {
        onUncertain?.()
        throw new RequestInterruptedError('请求中断或网络异常，操作结果未知；请刷新状态确认，勿直接重复操作')
      }
      throw error
    } finally {
      setActionPending(action, false)
    }
  }

  return { apiCall, pendingActions }
}
