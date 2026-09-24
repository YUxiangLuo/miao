import { useEffect, useState } from 'react'
import type { AiAnalytics as Analytics } from '../types/api'
import { Button, SectionCard } from './ui'

const outcomes: Record<string, string> = {
  success: '成功',
  cancelled: '客户端取消',
  http_error: '上游 HTTP 错误',
  api_error: '上游 API 错误',
  gateway_error: '网关不可用',
  gateway_stopped: '网关停止',
  connection_error: '连接失败',
  timeout: '超时',
  stream_interrupted: '流中断',
}
const count = (n: number) => n.toLocaleString()
const duration = (n: number | null) => (n === null ? '—' : `${(n / 1000).toFixed(2)} s`)

export function AiAnalytics({ token, view }: { token: string; view: 'overview' | 'requests' }) {
  const [days, setDays] = useState('1')
  const [provider, setProvider] = useState('')
  const [model, setModel] = useState('')
  const [outcome, setOutcome] = useState('')
  const [before, setBefore] = useState<number | null>(null)
  const [refresh, setRefresh] = useState(0)
  const [data, setData] = useState<Analytics | null>(null)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    setData(null)
    setError('')
    const query = new URLSearchParams({ days, provider, model, outcome })
    if (before !== null) query.set('before', String(before))
    void fetch(`/api/ai/analytics?${query}`, {
      cache: 'no-store',
      headers: { Authorization: `Bearer ${token}` },
      signal: controller.signal,
    })
      .then(async (response) => {
        if (!response.ok) throw new Error('统计读取失败，请重试或重新解锁。')
        const result = (await response.json()) as Analytics
        if (!controller.signal.aborted) setData(result)
      })
      .catch((cause: unknown) => {
        if (!controller.signal.aborted) setError(cause instanceof Error ? cause.message : '统计读取失败')
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false)
      })
    return () => controller.abort()
  }, [token, days, provider, model, outcome, before, refresh])

  return (
    <section className="ai-statistics" aria-label={view === 'overview' ? '用量概览' : '请求记录'}>
      <div className="ai-stat-filters">
        <label>
          时间范围
          <select
            value={days}
            onChange={(e) => {
              setDays(e.target.value)
              setBefore(null)
            }}
          >
            <option value="1">最近 24 小时</option>
            <option value="7">最近 7 天</option>
            <option value="30">最近 30 天</option>
          </select>
        </label>
        <label>
          供应商 ID
          <input
            value={provider}
            placeholder="全部供应商"
            onChange={(e) => {
              setProvider(e.target.value)
              setBefore(null)
            }}
          />
        </label>
        <label>
          模型别名
          <input
            value={model}
            placeholder="全部模型"
            onChange={(e) => {
              setModel(e.target.value)
              setBefore(null)
            }}
          />
        </label>
        <label>
          请求结果
          <select
            value={outcome}
            onChange={(e) => {
              setOutcome(e.target.value)
              setBefore(null)
            }}
          >
            <option value="">全部结果</option>
            {Object.entries(outcomes).map(([key, label]) => (
              <option key={key} value={key}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <Button
          tone="secondary"
          size="sm"
          disabled={loading}
          onClick={() => {
            setBefore(null)
            setRefresh((n) => n + 1)
          }}
        >
          刷新统计
        </Button>
      </div>
      <p className="ai-help">
        仅统计通过鉴权且模型有效的推理请求，在请求结束后写入。数据保留 30
        天，不保存提示词、响应正文或密钥。首字节耗时不等于首 token 延迟。窄屏下可横向滚动表格查看全部列。
      </p>
      {loading && <p role="status">读取统计中…</p>}
      {error && (
        <p role="alert" className="ai-error">
          {error}
        </p>
      )}
      {data && (
        <>
          {(data.collection_error || data.dropped_records > 0) && (
            <p role="alert" className="ai-error">
              {data.collection_error || '统计写入积压'}；本次运行已丢失 {count(data.dropped_records)}{' '}
              条记录，统计不完整。
            </p>
          )}
          <p className="ai-help">统计截至 {new Date(data.until).toLocaleString()} · 手动刷新</p>
          {view === 'overview' ? (
            <>
              <div className="ai-overview">
                <SectionCard>
                  <span className="ai-overview-label">请求数</span>
                  <strong>{count(data.totals.requests)}</strong>
                  <p>
                    成功 {count(data.totals.succeeded)} · 失败 {count(data.totals.failed)} · 取消{' '}
                    {count(data.totals.cancelled)}
                  </p>
                </SectionCard>
                <SectionCard>
                  <span className="ai-overview-label">已知用量 · Token</span>
                  <strong>{data.totals.usage_known ? count(data.totals.total_tokens) : '未知'}</strong>
                  <p>
                    完整用量 {count(data.totals.usage_known)} / {count(data.totals.requests)} 次请求
                  </p>
                  <p>
                    输入 {data.totals.usage_known ? count(data.totals.input_tokens) : '—'} · 输出{' '}
                    {data.totals.usage_known ? count(data.totals.output_tokens) : '—'}
                  </p>
                </SectionCard>
                <SectionCard>
                  <span className="ai-overview-label">请求总耗时</span>
                  <strong>{duration(data.totals.avg_duration_ms)}</strong>
                  <p>平均值 · P95 {duration(data.totals.p95_duration_ms)}</p>
                </SectionCard>
              </div>
              <p className="ai-help">
                仅汇总供应商返回且完整的用量，不完整或缺失不按零计量；这里不是账单，暂不估算费用。流式客户端可按供应商要求传入
                stream_options.include_usage。
              </p>
              <h3>供应商 / 模型用量（请求数前 100 组）</h3>
              <div className="ai-stat-table" tabIndex={0} role="region" aria-label="模型用量表">
                <table>
                  <thead>
                    <tr>
                      <th>供应商 / 模型</th>
                      <th>请求</th>
                      <th>成功 / 失败 / 取消</th>
                      <th>完整用量</th>
                      <th>已知 Token</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.groups.map((group) => (
                      <tr key={JSON.stringify([group.provider, group.model])}>
                        <td>
                          {group.provider}
                          <small>{group.model}</small>
                        </td>
                        <td>{count(group.requests)}</td>
                        <td>
                          {group.succeeded} / {group.failed} / {group.cancelled}
                        </td>
                        <td>
                          {group.usage_known} / {group.requests}
                        </td>
                        <td>{group.usage_known ? count(group.total_tokens) : '未知'}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          ) : (
            <>
              <div className="ai-stat-table" tabIndex={0} role="region" aria-label="请求明细表">
                <table>
                  <thead>
                    <tr>
                      <th>开始时间 / 接口</th>
                      <th>供应商 / 模型 → 上游模型</th>
                      <th>结果 / HTTP</th>
                      <th>总耗时 / 首字节</th>
                      <th>Token（输入 / 输出 / 总量）</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.records.map((row) => (
                      <tr key={row.id}>
                        <td>
                          {new Date(row.timestamp).toLocaleString()}
                          <small>
                            {row.endpoint}
                            {row.streaming ? ' · 流式' : ''}
                          </small>
                        </td>
                        <td>
                          {row.provider}
                          <small>
                            {row.model} → {row.upstream_model}
                          </small>
                        </td>
                        <td>
                          {outcomes[row.outcome] || row.outcome}
                          <small>{row.http_status ?? '未收到上游响应'}</small>
                        </td>
                        <td>
                          {duration(row.duration_ms)}
                          <small>{duration(row.first_byte_ms)}</small>
                        </td>
                        <td>
                          {row.total_tokens === null
                            ? '未知'
                            : `${count(row.input_tokens ?? 0)} / ${count(row.output_tokens ?? 0)} / ${count(row.total_tokens)}`}
                          <small>
                            {row.usage_complete
                              ? '完整'
                              : row.total_tokens !== null
                                ? '不完整，不计入汇总'
                                : '未返回完整用量'}
                          </small>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <div className="ai-stat-pages">
                <Button tone="secondary" size="sm" disabled={before === null} onClick={() => setBefore(null)}>
                  最新记录
                </Button>
                <Button
                  tone="secondary"
                  size="sm"
                  disabled={data.next_before === null}
                  onClick={() => setBefore(data.next_before)}
                >
                  更早记录
                </Button>
              </div>
            </>
          )}
          {data.totals.requests === 0 && <div className="empty-block">此范围内暂无请求记录</div>}
        </>
      )}
    </section>
  )
}
