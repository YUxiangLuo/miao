import { useEffect, useRef, useCallback } from 'react'
import { POLL_INTERVAL } from '../utils.js'

/**
 * 统一轮询管理 hook
 * 合并多个定时任务到单个定时器，减少资源消耗
 */
export function usePolling(tasks, enabled = true) {
  const tasksRef = useRef(tasks)
  const timerRef = useRef(null)
  const runningTaskIndexesRef = useRef(new Set())

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

    // 立即执行一次
    runTasks()

    // 设置定时器
    timerRef.current = window.setInterval(runTasks, POLL_INTERVAL)

    return () => {
      if (timerRef.current) {
        window.clearInterval(timerRef.current)
        timerRef.current = null
      }
    }
  }, [enabled, runTasks])

  // 返回手动触发函数
  return { triggerPoll: runTasks }
}
