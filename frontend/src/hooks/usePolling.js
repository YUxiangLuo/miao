import { useEffect, useRef, useCallback } from 'react'
import { POLL_INTERVAL, POLL_TASK_TIMEOUT } from '../utils.js'

/**
 * 统一轮询管理 hook
 * 合并多个定时任务到单个定时器，减少资源消耗
 * 每个任务都会收到 AbortSignal，网络任务应将其传给 fetch。
 */
export function usePolling(
  tasks,
  enabled = true,
  immediate = true,
  timeoutMs = POLL_TASK_TIMEOUT
) {
  const tasksRef = useRef(tasks)
  const timerRef = useRef(null)
  const runningRef = useRef(false)
  const activeControllerRef = useRef(null)

  // 保持 tasksRef 最新，避免定时器重建
  useEffect(() => {
    tasksRef.current = tasks
  }, [tasks])

  const runTasks = useCallback(async () => {
    if (runningRef.current) return false

    const currentTasks = tasksRef.current
    if (!Array.isArray(currentTasks) || currentTasks.length === 0) return false

    runningRef.current = true
    const controller = new AbortController()
    activeControllerRef.current = controller
    let timeoutId = null
    let handleAbort = null

    try {
      const tasksDone = Promise.allSettled(
        currentTasks.map((task) => Promise.resolve().then(() => task(controller.signal)))
      ).then(() => true)
      const aborted = new Promise((resolve) => {
        handleAbort = () => resolve(false)
        controller.signal.addEventListener('abort', handleAbort, { once: true })
      })

      timeoutId = window.setTimeout(() => controller.abort(), timeoutMs)
      return await Promise.race([tasksDone, aborted])
    } finally {
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId)
      }
      if (handleAbort) {
        controller.signal.removeEventListener('abort', handleAbort)
      }
      if (activeControllerRef.current === controller) {
        activeControllerRef.current = null
      }
      if (!controller.signal.aborted) {
        controller.abort()
      }
      runningRef.current = false
    }
  }, [timeoutMs])

  useEffect(() => {
    return () => {
      activeControllerRef.current?.abort()
      activeControllerRef.current = null
    }
  }, [])

  useEffect(() => {
    if (!enabled) {
      activeControllerRef.current?.abort()
      activeControllerRef.current = null
      if (timerRef.current) {
        window.clearTimeout(timerRef.current)
        timerRef.current = null
      }
      return
    }

    let cancelled = false

    const scheduleNext = () => {
      if (cancelled) return
      timerRef.current = window.setTimeout(async () => {
        await runTasks()
        scheduleNext()
      }, POLL_INTERVAL)
    }

    const start = async () => {
      if (immediate) await runTasks()
      scheduleNext()
    }

    start()

    return () => {
      cancelled = true
      activeControllerRef.current?.abort()
      activeControllerRef.current = null
      if (timerRef.current) {
        window.clearTimeout(timerRef.current)
        timerRef.current = null
      }
    }
  }, [enabled, immediate, runTasks])

  // 返回手动触发函数
  return { triggerPoll: runTasks }
}
