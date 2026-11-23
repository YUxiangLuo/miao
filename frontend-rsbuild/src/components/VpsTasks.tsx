import { For, Show, createEffect, createMemo, createSignal } from 'solid-js'
import { VpsErrorDetails } from './node-modal/VpsErrorDetails'
import type { VpsTask } from '../hooks/useVpsDeployment'

const PHASE_LABELS: Record<VpsTask['phase'], string> = {
  waiting: '等待部署结果',
  syncing: '同步节点列表',
  success: '已完成',
  error: '请求失败',
  unknown: '结果未知',
}

export function VpsTasks(props: { tasks: VpsTask[] }) {
  const [now, setNow] = createSignal(Date.now())
  const running = createMemo(() => props.tasks.some((task) => !task.finishedAt))

  createEffect(
    () => running(),
    (isRunning) => {
      if (!isRunning) return
      setNow(Date.now())
      const timer = window.setInterval(() => setNow(Date.now()), 1000)
      return () => window.clearInterval(timer)
    },
  )

  return (
    <Show when={props.tasks.length > 0}>
      <details class="panel-card vps-tasks">
        <summary>
          VPS 部署记录 · {PHASE_LABELS[props.tasks[0].phase]} · {props.tasks.length} 条
        </summary>
        <p>保留最近 10 次记录，刷新或关闭页面后清空。关闭添加窗口不取消部署。</p>
        <div class="vps-tasks-list">
          <For each={props.tasks} keyed={(task) => task.id}>
            {(task) => (
              <article aria-label={`部署 ${task().host}`}>
                <strong>{task().host} · {PHASE_LABELS[task().phase]}</strong>
                <div class="num">
                  {new Date(task().startedAt).toLocaleString()} · {Math.max(0, Math.floor(((task().finishedAt ?? now()) - task().startedAt) / 1000))} 秒
                </div>
                <VpsErrorDetails text={task().message} />
                <Show when={task().phase === 'unknown' || task().phase === 'error'}>
                  <p>远端可能已变更，请核对本地节点和远端服务后再重试。</p>
                </Show>
              </article>
            )}
          </For>
        </div>
      </details>
    </Show>
  )
}
