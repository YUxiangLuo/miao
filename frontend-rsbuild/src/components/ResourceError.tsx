import { Show, createSignal } from 'solid-js'
import { Button } from './ui'

export function ResourceError(props: {
  label: string
  error: string
  hasData: boolean
  onRetry?: () => Promise<unknown>
}) {
  const [retrying, setRetrying] = createSignal(false)

  return (
    <div class="resource-error" role="alert">
      <strong>{props.label}读取失败</strong>
      <p>{props.hasData ? '保留上次读取的数据，等待更新。' : '列表状态未知，不代表配置为空。'}将自动重试。</p>
      <Show when={props.onRetry}>
        {(retry) => (
          <Button
            tone="secondary"
            size="sm"
            loading={retrying()}
            onClick={async () => {
              setRetrying(true)
              try {
                await retry()()
              } finally {
                setRetrying(false)
              }
            }}
          >
            重新读取{props.label}
          </Button>
        )}
      </Show>
      <details>
        <summary>错误详情</summary>
        <p>{props.error}</p>
      </details>
    </div>
  )
}
