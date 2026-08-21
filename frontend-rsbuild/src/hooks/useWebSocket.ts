import { useEffect, useRef, useCallback } from 'react'

/**
 * 健壮的 WebSocket hook
 * - 指数退避重连（延迟封顶，无次数上限，桌面壳没有手动刷新手段）
 * - 页面可见性感知
 * - 正常/异常关闭区分处理
 */
export function useWebSocket<T = unknown>(url: string, onMessage: (data: T) => void, enabled = true) {
  const wsRef = useRef<WebSocket | null>(null)
  const retryCountRef = useRef(0)
  const retryTimerRef = useRef<number | null>(null)
  const onMessageRef = useRef(onMessage)
  const enabledRef = useRef(enabled)

  // 保持 ref 最新
  useEffect(() => {
    onMessageRef.current = onMessage
  }, [onMessage])

  useEffect(() => {
    enabledRef.current = enabled
  }, [enabled])

  const BASE_DELAY = 1000
  const MAX_DELAY = 15000

  const getRetryDelay = useCallback(() => {
    // 指数退避：1s, 2s, 4s, 8s, 15s(max)
    const delay = Math.min(BASE_DELAY * Math.pow(2, retryCountRef.current), MAX_DELAY)
    // 添加随机抖动，避免多个客户端同时重连
    return delay + Math.random() * 500
  }, [])

  const cleanup = useCallback(() => {
    if (retryTimerRef.current) {
      window.clearTimeout(retryTimerRef.current)
      retryTimerRef.current = null
    }
    if (wsRef.current) {
      // 移除事件处理器，避免触发 onclose 中的重连逻辑
      wsRef.current.onclose = null
      wsRef.current.onerror = null
      wsRef.current.onmessage = null
      wsRef.current.onopen = null
      if (
        wsRef.current.readyState === WebSocket.CONNECTING
        || wsRef.current.readyState === WebSocket.OPEN
      ) {
        wsRef.current.close(1000, 'Normal closure')
      }
      wsRef.current = null
    }
  }, [])

  const connect = useCallback(() => {
    if (!enabledRef.current) return
    if (wsRef.current && wsRef.current.readyState === WebSocket.CONNECTING) return

    cleanup()

    try {
      const ws = new WebSocket(url)
      wsRef.current = ws

      ws.onopen = () => {
        // 连接成功，重置重试计数
        retryCountRef.current = 0
      }

      ws.onmessage = (event) => {
        try {
          const data = JSON.parse(event.data) as T
          onMessageRef.current?.(data)
        } catch {
          // ignore parse errors
        }
      }

      ws.onclose = () => {
        wsRef.current = null

        // cleanup 会先移除 onclose，因此能走到这里的关闭都来自远端。
        // sing-box 重载可能正常关闭(code 1000)上游；只要功能仍启用就应重连。
        if (!enabledRef.current) {
          return
        }

        // 指数退避重连（无次数上限；退避延迟封顶在 MAX_DELAY）
        const delay = getRetryDelay()
        retryCountRef.current++
        retryTimerRef.current = window.setTimeout(() => {
          if (enabledRef.current) {
            connect()
          }
        }, delay)
      }

      ws.onerror = () => {
        // onerror 之后通常会触发 onclose，重连逻辑在 onclose 中处理
      }
    } catch {
      // WebSocket 构造失败（如无效 URL）
    }
  }, [url, cleanup, getRetryDelay])

  // 页面可见性变化处理
  useEffect(() => {
    const handleVisibilityChange = () => {
      if (document.hidden) {
        // 页面不可见时，暂停重连（保留现有连接）
        if (retryTimerRef.current) {
          window.clearTimeout(retryTimerRef.current)
          retryTimerRef.current = null
        }
      } else {
        // 页面可见时，如果连接断开则立即重连
        if (enabledRef.current && !wsRef.current) {
          retryCountRef.current = 0 // 重置重试计数
          connect()
        }
      }
    }

    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => document.removeEventListener('visibilitychange', handleVisibilityChange)
  }, [connect])

  // 主连接管理
  useEffect(() => {
    if (enabled) {
      connect()
    } else {
      cleanup()
    }

    return cleanup
  }, [enabled, connect, cleanup])

  return { close: cleanup }
}
