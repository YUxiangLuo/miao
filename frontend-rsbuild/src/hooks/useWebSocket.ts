import { createEffect, createMemo, type Accessor } from 'solid-js'

const BASE_DELAY = 1000
const MAX_DELAY = 15000

/**
 * 健壮的 WebSocket 原语
 * - 指数退避重连（延迟封顶，无次数上限）
 * - 页面可见性感知
 * - 正常/异常关闭区分处理
 */
export function useWebSocket<T = unknown>(
  url: Accessor<string> | string,
  onMessage: (data: T) => void,
  enabled: Accessor<boolean> | boolean = true,
) {
  const getUrl = typeof url === 'function' ? url : () => url
  const getEnabled = typeof enabled === 'function' ? enabled : () => enabled

  let ws: WebSocket | null = null
  let retryCount = 0
  let retryTimer: number | null = null
  let isEnabled = false
  let currentUrl = ''

  const getRetryDelay = () => {
    const delay = Math.min(BASE_DELAY * Math.pow(2, retryCount), MAX_DELAY)
    return delay + Math.random() * 500
  }

  const cleanup = () => {
    if (retryTimer !== null) {
      window.clearTimeout(retryTimer)
      retryTimer = null
    }
    if (ws) {
      ws.onclose = null
      ws.onerror = null
      ws.onmessage = null
      ws.onopen = null
      if (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN) {
        ws.close(1000, 'Normal closure')
      }
      ws = null
    }
  }

  const connect = () => {
    if (!isEnabled || document.hidden || !currentUrl) return
    if (ws && ws.readyState === WebSocket.CONNECTING) return

    cleanup()

    try {
      const socket = new WebSocket(currentUrl)
      ws = socket

      socket.onopen = () => {
        retryCount = 0
      }

      socket.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data) as T
          onMessage(data)
        } catch {
          // ignore parse errors
        }
      }

      socket.onclose = () => {
        ws = null
        if (!isEnabled || document.hidden) return

        const delay = getRetryDelay()
        retryCount++
        retryTimer = window.setTimeout(() => {
          if (isEnabled) connect()
        }, delay)
      }

      socket.onerror = () => {
        // onclose handles reconnection
      }
    } catch {
      // ignore invalid URL errors
    }
  }

  const targetUrlMemo = createMemo(() => getUrl())
  const activeMemo = createMemo(() => getEnabled())

  createEffect(
    () => ({ targetUrl: targetUrlMemo(), active: activeMemo() }),
    ({ targetUrl, active }) => {
      currentUrl = targetUrl
      isEnabled = active

      if (active) {
        connect()
      } else {
        cleanup()
      }

      const handleVisibilityChange = () => {
        if (document.hidden) {
          if (retryTimer !== null) {
            window.clearTimeout(retryTimer)
            retryTimer = null
          }
        } else if (isEnabled && !ws) {
          retryCount = 0
          connect()
        }
      }

      document.addEventListener('visibilitychange', handleVisibilityChange)
      return () => {
        document.removeEventListener('visibilitychange', handleVisibilityChange)
        cleanup()
      }
    },
  )

  return { close: cleanup }
}
