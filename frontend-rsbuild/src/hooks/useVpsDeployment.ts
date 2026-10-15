import { useCallback, useRef, useState } from 'react'
import type { useAppData } from './useAppData'
import { RequestInterruptedError } from './request'
import { mutationFeedback } from './mutationFeedback'
import type { VpsDeployRequest, VpsDeployResponse } from '../types/api'

export interface VpsTask {
  id: number
  host: string
  startedAt: number
  finishedAt?: number
  phase: 'waiting' | 'syncing' | 'success' | 'error' | 'unknown'
  message: string
}

export function useVpsDeployment({ apiCall, fetchNodes, clearDelays, showToast }: Pick<ReturnType<typeof useAppData>, 'apiCall' | 'fetchNodes' | 'clearDelays' | 'showToast'>) {
  const [vpsTasks, setVpsTasks] = useState<VpsTask[]>([])
  const sequence = useRef(0)
  const busy = useRef(false)
  const handleDeployVps = useCallback(async ({ ip, password }: VpsDeployRequest): Promise<boolean> => {
    if (busy.current) return false
    busy.current = true
    const id = ++sequence.current
    const task: VpsTask = { id, host: ip.trim(), startedAt: Date.now(), phase: 'waiting', message: '等待后端部署结果；当前接口不提供 SSH 内部进度。' }
    setVpsTasks(tasks => [task, ...tasks].slice(0, 10))
    const update = (patch: Partial<VpsTask>) => setVpsTasks(tasks => tasks.map(task => task.id === id ? { ...task, ...patch } : task))
    try {
      const payload = await apiCall<VpsDeployResponse>('vps/deploy', {
        method: 'POST', body: JSON.stringify({ ip, password } satisfies VpsDeployRequest),
      }, 'deployVps')
      update({ phase: 'syncing', message: payload.message || '部署接口已确认完成，正在同步节点列表。' })
      const nodes = await fetchNodes()
      clearDelays()
      const feedback = mutationFeedback(payload.message || '部署接口已确认完成', nodes, '部署')
      update({ phase: 'success', message: feedback.message, finishedAt: Date.now() })
      showToast(feedback.message, feedback.tone)
      return true
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      const uncertain = error instanceof RequestInterruptedError || error instanceof TypeError
      update({ phase: uncertain ? 'unknown' : 'error', message, finishedAt: Date.now() })
      // The pane may already have unmounted. The page-level record owns the result.
      showToast(uncertain ? 'VPS 部署结果未知，请查看部署记录并核对远端状态。' : 'VPS 部署请求失败，详情已保留在部署记录。', 'error')
      throw error
    } finally {
      busy.current = false
    }
  }, [apiCall, fetchNodes, clearDelays, showToast])
  return { vpsTasks, vpsDeploying: vpsTasks.some(task => task.finishedAt === undefined), handleDeployVps }
}
