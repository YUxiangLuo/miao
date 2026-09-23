import { useEffect, useState } from 'react'
import { VpsErrorDetails } from './node-modal/VpsErrorDetails'
import type { VpsTask } from '../hooks/useVpsDeployment'

const PHASE_LABELS: Record<VpsTask['phase'], string> = {
  waiting: '等待部署结果', syncing: '同步节点列表', success: '已完成', error: '请求失败', unknown: '结果未知',
}

export function VpsTasks({ tasks }: { tasks: VpsTask[] }) {
  const [now, setNow] = useState(Date.now)
  const running = tasks.some(task => !task.finishedAt)
  useEffect(() => {
    if (!running) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [running])
  if (!tasks.length) return null
  return (
    <details className="panel-card vps-tasks">
      <summary>VPS 部署记录 · {PHASE_LABELS[tasks[0].phase]} · {tasks.length} 条</summary>
      <p>保留最近 10 次记录，刷新或关闭页面后清空。关闭添加窗口不取消部署。</p>
      <div className="vps-tasks-list">
        {tasks.map(task => (
          <article key={task.id} aria-label={`部署 ${task.host}`}>
            <strong>{task.host} · {PHASE_LABELS[task.phase]}</strong>
            <div>{new Date(task.startedAt).toLocaleString()} · {Math.max(0, Math.floor(((task.finishedAt ?? now) - task.startedAt) / 1000))} 秒</div>
            <VpsErrorDetails text={task.message} />
            {(task.phase === 'unknown' || task.phase === 'error') && <p>远端可能已变更，请核对本地节点和远端服务后再重试。</p>}
          </article>
        ))}
      </div>
    </details>
  )
}
