import { createEffect, createMemo, untrack, type Accessor } from 'solid-js'
import { POLL_INTERVAL } from '../utils'

// 返回值被轮询器忽略，允许任意签名（Promise 或非 Promise）
export type PollTask = () => unknown

/**
 * 统一轮询管理原语
 * 合并多个定时任务到单个定时器，减少资源消耗。
 * interval 变化时会重建定时器并立即补跑一轮。
 */
export function usePolling(
  tasks: Accessor<PollTask[]> | PollTask[],
  enabled: Accessor<boolean> | boolean = true,
  interval: Accessor<number> | number = POLL_INTERVAL,
) {
  const getTasks = typeof tasks === 'function' ? tasks : () => tasks
  const getEnabled = typeof enabled === 'function' ? enabled : () => enabled
  const getInterval = typeof interval === 'function' ? interval : () => interval

  let timer: number | null = null
  const runningTasks = new Set<PollTask>()

  const runTasks = () => {
    const currentTasks = untrack(getTasks)
    if (!Array.isArray(currentTasks) || currentTasks.length === 0) {
      return Promise.resolve([])
    }

    const startedTasks = currentTasks.flatMap((task) => {
      if (runningTasks.has(task)) return []

      runningTasks.add(task)
      const finish = () => {
        runningTasks.delete(task)
      }
      let result: unknown
      try {
        result = task()
      } catch {
        finish()
        return []
      }
      const promise = Promise.resolve(result).then(finish, finish)
      return [promise]
    })

    return Promise.allSettled(startedTasks)
  }

  const activeMemo = createMemo(() => getEnabled())
  const intervalMemo = createMemo(() => getInterval())

  createEffect(
    () => ({ active: activeMemo(), ms: intervalMemo() }),
    ({ active, ms }) => {
      const stopTimer = () => {
        if (timer !== null) {
          window.clearInterval(timer)
          timer = null
        }
      }

      if (!active) {
        stopTimer()
        return
      }

      const startTimer = () => {
        stopTimer()
        void runTasks()
        timer = window.setInterval(runTasks, ms)
      }

      if (!document.hidden) startTimer()

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
    },
  )
}
