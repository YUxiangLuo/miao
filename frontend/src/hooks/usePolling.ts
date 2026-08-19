import { useEffect, useRef, useCallback } from 'react'
import { POLL_INTERVAL } from '../utils'

// 返回值被轮询器忽略，允许任意签名（Promise 或非 Promise）
export type PollTask = () => unknown

/**
 * 统一轮询管理 hook
 * 合并多个定时任务到单个定时器，减少资源消耗。
 * interval 变化时会重建定时器并立即补跑一轮（启动期加速轮询靠它生效）。
 */
export function usePolling(tasks: PollTask[], enabled = true, interval = POLL_INTERVAL) {
  const tasksRef = useRef(tasks)
  const timerRef = useRef<number | null>(null)
  const runningTaskIndexesRef = useRef(new Set<number>())

  // 保持 tasksRef 最新，避免定时器重建
  useEffect(() => {
    tasksRef.current = tasks
  }, [tasks])

  const runTasks = useCallback(() => {
    const currentTasks = tasksRef.current
    if (!Array.isArray(currentTasks) || currentTasks.length === 0) {
      return Promise.resolve([])
    }

    const startedTasks = currentTasks.flatMap((task, index) => {
      if (runningTaskIndexesRef.current.has(index)) return []

      runningTaskIndexesRef.current.add(index)
      const promise = Promise.resolve()
        .then(() => task())
        .catch(() => undefined)
        .finally(() => runningTaskIndexesRef.current.delete(index))
      return [promise]
    })

    return Promise.allSettled(startedTasks)
  }, [])

  useEffect(() => {
    if (!enabled) {
      if (timerRef.current) {
        window.clearInterval(timerRef.current)
        timerRef.current = null
      }
      return
    }

    const stopTimer = () => {
      if (timerRef.current) {
        window.clearInterval(timerRef.current)
        timerRef.current = null
      }
    }
    const startTimer = () => {
      stopTimer()
      runTasks()
      timerRef.current = window.setInterval(runTasks, interval)
    }

    startTimer()

    // 页面隐藏时暂停轮询，恢复可见时立即补一次
    const handleVisibilityChange = () => {
      if (document.hidden) {
        stopTimer()
      } else {
        startTimer()
      }
    }
    document.addEventListener('visibilitychange', handleVisibilityChange)

    return () => {
      stopTimer()
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [enabled, interval, runTasks])

  // 返回手动触发函数
  return { triggerPoll: runTasks }
}
