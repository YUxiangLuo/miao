import { useState } from 'react'
import { Button } from './ui'

export function ResourceError({ label, error, hasData, onRetry }: {
  label: string
  error: string
  hasData: boolean
  onRetry?: () => Promise<unknown>
}) {
  const [retrying, setRetrying] = useState(false)
  return (
    <div className="resource-error" role="alert">
      <strong>{label}读取失败</strong>
      <p>{hasData ? '保留上次读取的数据，等待更新。' : '列表状态未知，不代表配置为空。'}将自动重试。</p>
      {onRetry && <Button tone="secondary" size="sm" loading={retrying} onClick={async () => {
        setRetrying(true)
        try { await onRetry() } finally { setRetrying(false) }
      }}>重新读取{label}</Button>}
      <details><summary>错误详情</summary><p>{error}</p></details>
    </div>
  )
}
