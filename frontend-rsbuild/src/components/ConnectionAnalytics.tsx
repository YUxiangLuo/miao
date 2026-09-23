import { useCallback, useEffect, useMemo, useState } from 'react'
import type { FormEvent } from 'react'
import { RefreshCw } from 'lucide-react'
import { fetchJson } from '../hooks/request'
import { useLatestRequest } from '../hooks/useLatestRequest'
import { ICON } from '../tokens'
import { formatBytes } from '../utils'
import type { AnalyticsLogs, AnalyticsSummary, ApiResponse, LogFacet } from '../types/api'

export type Hours = 1 | 6 | 24
export type LogSelection = { hours?: Hours; q?: string; level?: string; component?: string; event?: string; context?: string; id?: number }
const HOURS: Hours[] = [1, 6, 24]
const DIMENSIONS = [
  ['domain', '域名'], ['source', '来源'], ['process', '进程'], ['outbound', '出口'],
  ['rule', '规则'], ['network', '网络'],
] as const
const LEVELS = ['', 'error', 'warn', 'info', 'debug', 'fatal', 'panic', 'trace', 'unknown'] as const

function responseData<T>(response: ApiResponse<T>): T {
  if (!response.success || response.data == null) throw new Error(response.message || '请求失败')
  return response.data
}

function HoursPicker({ value, onChange }: { value: Hours; onChange: (hours: Hours) => void }) {
  return <div className="connections-pills" aria-label="分析时段">{HOURS.map(hours => (
    <button type="button" key={hours} className={`connections-pill ${value === hours ? 'active' : ''}`}
      aria-pressed={value === hours} onClick={() => onChange(hours)}>{hours} 小时</button>
  ))}</div>
}

const duration = (value: number | null) => value == null ? '—' : `${value.toFixed(value < 10 ? 2 : 1)} ms`
const time = (value: number) => new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
const SECTIONS = [['overview', '活动概览'], ['dns', 'DNS 行为'], ['routes', '分流与出口'], ['traffic', '流量采样']] as const

/** All charts share bucket boundaries and a hover cursor, like a metrics workspace. */
function Timeline({ title, values, timestamps, unit, active, onActive, tone = 'info' }: {
  title: string; values: Array<number | null>; timestamps: number[]; unit: string; active: number | null;
  onActive: (value: number | null) => void; tone?: string;
}) {
  const peak = Math.max(0, ...values.filter((v): v is number => v != null))
  const max = Math.max(1, peak)
  const point = (value: number, index: number) => `${index * 1000 / Math.max(1, values.length - 1)},${110 - value / max * 100}`
  const points = values.map((value, index) => value == null ? '' : point(value, index)).join(' ')
  const line = values.map((value, index) => value == null ? '' : `${index === 0 || values[index - 1] == null ? 'M' : 'L'}${point(value, index)}`).join(' ')
  return <section className={`analytics-section analytics-timeline tone-${tone}`}>
    <div className="analytics-section-head"><h4>{title}</h4><span>{active == null ? `峰值 ${values.some(v => v != null) ? peak.toLocaleString() : '—'} ${unit}` : `${values[active]?.toLocaleString() ?? '—'} ${unit} · ${time(timestamps[active])}`}</span></div>
    <svg viewBox="0 0 1000 120" preserveAspectRatio="none" role="img" aria-label={`${title}，单位 ${unit}`} onPointerLeave={() => onActive(null)} onPointerMove={event => {
      const rect = event.currentTarget.getBoundingClientRect()
      if (!values.length) return
      onActive(Math.max(0, Math.min(values.length - 1, Math.round((event.clientX - rect.left) / rect.width * (values.length - 1)))))
    }}>
      <path className="chart-grid" d="M0 10H1000 M0 60H1000 M0 110H1000" />
      {!values.includes(null) && <polygon className="chart-fill" points={`0,110 ${points} 1000,110`} />}
      <path className="chart-line" d={line} />
      {active != null && <line className="chart-cursor" x1={active * 1000 / Math.max(1, values.length - 1)} x2={active * 1000 / Math.max(1, values.length - 1)} y1="0" y2="120" />}
    </svg>
    <footer><span>{timestamps.length ? time(timestamps[0]) : ''}</span><span>每格 5 分钟</span><span>{timestamps.length ? time(timestamps[timestamps.length - 1]) : ''}</span></footer>
  </section>
}

function Facets({ title, facets, dimension, onExplore }: { title: string; facets: LogFacet[]; dimension: string; onExplore: (selection: LogSelection) => void }) {
  const rows = facets.filter(row => row.dimension === dimension)
  const max = Math.max(1, ...rows.map(row => row.count))
  return <section className="analytics-section"><h4>{title}<small>日志记录数 · Top 20</small></h4>
    {rows.length ? <ol className="analytics-facets">{rows.map(row => <li key={row.name}>
      <button onClick={() => onExplore(dimension === 'route' ? { q: row.name, event: 'route.match' } : { [dimension]: row.name })} title="查看对应原始日志">
        <span className="facet-bar" style={{ width: `${row.count / max * 100}%` }} /><span>{row.name}</span><strong>{row.count.toLocaleString()}</strong>
      </button></li>)}</ol> : <p className="analytics-empty">该时段暂无记录</p>}
  </section>
}

export function AnalyticsSummaryPane({ visible, onExplore }: { visible: boolean; onExplore: (selection: LogSelection) => void }) {
  const [hours, setHours] = useState<Hours>(24)
  const [summary, setSummary] = useState<AnalyticsSummary | null>(null)
  const [dimension, setDimension] = useState<(typeof DIMENSIONS)[number][0]>('domain')
  const [section, setSection] = useState<(typeof SECTIONS)[number][0]>('overview')
  const [dnsDimension, setDnsDimension] = useState('domain')
  const [active, setActive] = useState<number | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const { begin, cancel } = useLatestRequest()
  const load = useCallback(async () => {
    const current = begin()
    setLoading(true)
    try {
      const value = responseData(await fetchJson<ApiResponse<AnalyticsSummary>>(`/api/analytics/summary?hours=${hours}`, { signal: current.signal }))
      if (current.isCurrent()) { setSummary(value); setError('') }
    } catch (reason) {
      if (current.isCurrent()) setError(reason instanceof Error ? reason.message : '分析加载失败')
    } finally {
      if (current.isCurrent()) setLoading(false)
    }
  }, [hours, begin])

  useEffect(() => {
    if (!visible) { cancel(); return }
    setSummary(null); setActive(null)
    void load()
    const timer = window.setInterval(() => { if (!document.hidden) void load() }, 10_000)
    return () => { window.clearInterval(timer); cancel() }
  }, [visible, load, cancel])

  const groups = useMemo(() => summary?.groups.filter(group => group.dimension === dimension).slice(0, 20) ?? [], [summary, dimension])
  const dnsBuckets = new Map(summary?.dns.timeline.map(bucket => [bucket.timestamp, bucket]))
  const timestamps = summary?.timeline.map(bucket => bucket.timestamp) ?? []
  const chartProps = { timestamps, active, onActive: setActive }
  const explore = (selection: LogSelection) => onExplore({ hours, ...selection })
  return <div className="analytics-workspace">
    <aside className="analytics-nav"><strong>网络活动分析</strong><small>sing-box · 最近 24 小时</small>{SECTIONS.map(([key, label]) => <button key={key} aria-current={section === key ? 'page' : undefined} onClick={() => { setSection(key); setActive(null) }}>{label}</button>)}<p>从时间趋势到事件原文<br />点击排行即可下钻</p></aside>
    <div className="analytics-pane">
    <div className="analytics-controls"><div><h3>{SECTIONS.find(([key]) => key === section)?.[1]}</h3><small>日志驱动 · 5 分钟分桶 · 每 10 秒刷新</small></div><HoursPicker value={hours} onChange={setHours} />
      <button className="btn btn-sm btn-secondary" onClick={() => void load()} disabled={loading}><RefreshCw size={ICON.sm} />刷新</button></div>
    {error && <div className="connections-error" role="alert">{error}</div>}
    {loading && !summary ? <div className="connections-empty inline">分析加载中…</div> : summary ? <>
      {summary.collection_error && <div className="connections-error">采集提示：{summary.collection_error}</div>}
      {section === 'overview' && <>
        <div className="analytics-metrics">
          <article><span>日志活动</span><strong>{summary.log_count.toLocaleString()}</strong><small>已索引 {summary.activity.parsed_count.toLocaleString()} 条</small></article>
          <article><span>关联上下文</span><strong>{summary.activity.contexts.toLocaleString()}</strong><small>按内核运行批次隔离，不等同连接数</small></article>
          <article><span>DNS 完成事件</span><strong>{summary.dns.queries.toLocaleString()}</strong><small>{summary.dns.groups.filter(row => row.dimension === 'domain').length ? '可查看域名、结果与延迟分布' : '等待内核产生查询'}</small></article>
        </div>
        <div className="analytics-grid">
          <Timeline {...chartProps} title="日志活动趋势" values={summary.timeline.map(b => b.log_count)} unit="条" />
          <Timeline {...chartProps} title="错误级别日志趋势" values={summary.timeline.map(b => b.error_count)} unit="条" tone="danger" />
          <Timeline {...chartProps} title="DNS 活动趋势" values={timestamps.map(t => dnsBuckets.get(t)?.queries ?? 0)} unit="次" />
          <Timeline {...chartProps} title="DNS P95 延迟" values={timestamps.map(t => dnsBuckets.get(t)?.p95_ms ?? null)} unit="ms" tone="accent" />
          <Facets title="组件分布" facets={summary.activity.facets} dimension="component" onExplore={explore} />
          <Facets title="事件分布" facets={summary.activity.facets} dimension="event" onExplore={explore} />
          <Facets title="日志级别" facets={summary.activity.facets} dimension="level" onExplore={explore} />
        </div>
        <p className="analytics-note">覆盖正常访问与错误事件；日志条数不等于请求数。未识别的格式保留原文，错误日志级别不代表连接失败率。</p>
      </>}
      {section === 'dns' && <>
        <div className="analytics-metrics">
          <article><span>DNS 完成事件</span><strong>{summary.dns.queries.toLocaleString()}</strong><small>非成功 {summary.dns.failures.toLocaleString()} 次（含 DNS RCODE）</small></article>
          <article><span>P50 / P95</span><strong>{duration(summary.dns.p50_ms)}</strong><small>P95 {duration(summary.dns.p95_ms)}</small></article>
          <article><span>P99 / 最慢</span><strong>{duration(summary.dns.p99_ms)}</strong><small>最慢 {duration(summary.dns.max_ms)}</small></article>
        </div>
        {summary.dns.queries === 0 ? <p className="analytics-empty">暂无结构化 DNS 完成事件。需要 miao.9 或更新内核产生查询后才能统计；旧文本日志不会被猜测成延迟数据。</p> : <>
          <div className="analytics-grid"><Timeline {...chartProps} title="DNS 活动趋势" values={timestamps.map(t => dnsBuckets.get(t)?.queries ?? 0)} unit="次" /><Timeline {...chartProps} title="DNS P95 延迟" values={timestamps.map(t => dnsBuckets.get(t)?.p95_ms ?? null)} unit="ms" tone="accent" /></div>
          <section className="analytics-section"><h4>最慢 DNS 事件<small>单次真实耗时 · Top 20 · 含失败</small></h4><div className="analytics-table-wrap"><table><thead><tr><th>域名 / 时间</th><th>类型</th><th>耗时</th><th>结果</th><th>上游</th><th>事件</th></tr></thead><tbody>{summary.dns.slowest.map(row => <tr key={row.log_id}>
            <td><button className="analytics-link" onClick={() => explore({ q: row.domain, component: 'dns' })}>{row.domain || '（空查询）'}</button><small>{new Date(row.timestamp).toLocaleString()}</small></td><td>{row.query_type}</td><td>{duration(row.duration_ms)}</td><td title={row.error ?? ''}>{row.outcome}{row.rcode != null ? ` · ${row.rcode}` : ''}</td><td>{row.transport ?? '未记录'}</td><td><button className="analytics-link" onClick={() => explore({ id: row.log_id })}>原文</button>{row.context && <button className="analytics-link" onClick={() => explore({ context: row.context })}>上下文</button>}</td>
          </tr>)}</tbody></table></div></section>
          <section className="analytics-section"><div className="analytics-section-head"><h4>DNS 分布</h4><select aria-label="DNS 分析维度" value={dnsDimension} onChange={event => setDnsDimension(event.target.value)}>{[['domain', '域名'], ['query_type', '查询类型'], ['transport', '上游'], ['outcome', '结果']].map(([key, label]) => <option key={key} value={key}>{label}</option>)}</select></div><div className="analytics-table-wrap"><table><thead><tr><th>维度</th><th>次数</th><th>非成功</th><th>P95</th><th>最慢</th></tr></thead><tbody>{summary.dns.groups.filter(row => row.dimension === dnsDimension).map(row => <tr key={row.name}><td>{row.name}</td><td>{row.queries}</td><td>{row.failures}</td><td>{duration(row.p95_ms)}</td><td>{duration(row.max_ms)}</td></tr>)}</tbody></table></div></section>
        </>}
        <p className="analytics-note">耗时来自内核单调时钟，包含路由、缓存及等待时间。LOOKUP 是一次完整解析，可能包含 A/AAAA，不能当作单个上游请求；未完成的查询不在完成事件中。缓存日志可能逐条记录答案，不能据此计算缓存命中率。</p>
      </>}
      {section === 'routes' && <><div className="analytics-grid"><Facets title="规则命中记录" facets={summary.activity.facets} dimension="route" onExplore={explore} /><Facets title="入站与出口活动" facets={summary.activity.facets.filter(f => f.name.startsWith('inbound/') || f.name.startsWith('outbound/'))} dimension="component" onExplore={explore} /></div><p className="analytics-note">每次日志记录为一次活动，规则匹配可能包含 DNS 与连接路由，不等同最终连接数。点击记录查看规则、嗅探、出站及后续事件。</p></>}
      {section === 'traffic' && <>
        <div className="analytics-metrics"><article><span>采样流量</span><strong>↓ {formatBytes(summary.download)}</strong><small>↑ {formatBytes(summary.upload)}</small></article><article><span>采样连接</span><strong>{summary.sampled_connections.toLocaleString()}</strong><small>按连接 ID 去重</small></article><article><span>最近采样</span><strong>{summary.last_sample_at ? time(summary.last_sample_at) : '—'}</strong><small>独立于日志事件的数据来源</small></article></div>
        <div className="analytics-grid"><Timeline {...chartProps} title="下载采样增量" values={summary.timeline.map(b => b.download)} unit="B" /><Timeline {...chartProps} title="上传采样增量" values={summary.timeline.map(b => b.upload)} unit="B" tone="accent" /></div>
        <section className="analytics-section"><div className="analytics-section-head"><h4>流量排行</h4>
        <div className="connections-pills">{DIMENSIONS.map(([key, label]) => <button key={key} className={`connections-pill ${dimension === key ? 'active' : ''}`} onClick={() => setDimension(key)}>{label}</button>)}</div></div>
        {groups.length ? <ol className="analytics-groups">{groups.map(group => <li key={`${group.dimension}:${group.name}`}><span title={group.name}>{group.name || '（未知）'}</span><em>{group.connections} 个连接</em><strong>↓ {formatBytes(group.download)} · ↑ {formatBytes(group.upload)}</strong></li>)}</ol> : <p className="analytics-empty">该维度暂无数据</p>}
        </section><p className="analytics-note">Clash API 每 3 秒采样，可能遗漏短连接和连接结束前的最后字节；流量为采样增量，并非精确网络计费。</p>
      </>}
    </> : null}
  </div></div>
}

export function AnalyticsLogsPane({ visible, initial = {} }: { visible: boolean; initial?: LogSelection }) {
  const [hours, setHours] = useState<Hours>(initial.hours ?? 24)
  const [level, setLevel] = useState(initial.level ?? '')
  const [input, setInput] = useState(initial.q ?? '')
  const [query, setQuery] = useState(initial.q ?? '')
  const [selection, setSelection] = useState(initial)
  const [before, setBefore] = useState<number | null>(null)
  const [logs, setLogs] = useState<AnalyticsLogs | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const { begin, cancel } = useLatestRequest()
  const load = useCallback(async () => {
    const current = begin(); setLoading(true)
    const params = new URLSearchParams({ hours: String(hours), limit: '100' })
    if (level) params.set('level', level)
    if (query) params.set('q', query)
    for (const key of ['component', 'event', 'context', 'id'] as const) if (selection[key] != null && selection[key] !== '') params.set(key, String(selection[key]))
    if (before != null) params.set('before', String(before))
    try {
      const value = responseData(await fetchJson<ApiResponse<AnalyticsLogs>>(`/api/analytics/logs?${params}`, { signal: current.signal }))
      if (current.isCurrent()) { setLogs(value); setError('') }
    } catch (reason) {
      if (current.isCurrent()) setError(reason instanceof Error ? reason.message : '日志加载失败')
    } finally { if (current.isCurrent()) setLoading(false) }
  }, [hours, level, query, before, begin, selection])
  useEffect(() => { if (visible) void load(); else cancel(); return cancel }, [visible, load, cancel])
  const reset = (change: () => void) => { setBefore(null); setLogs(null); change() }
  const submit = (event: FormEvent) => { event.preventDefault(); if (query === input.trim() && before == null) void load(); else reset(() => setQuery(input.trim())) }
  return <div className="analytics-pane logs-pane">
    <form className="analytics-log-controls" onSubmit={submit}>
      <HoursPicker value={hours} onChange={value => { if (value !== hours) reset(() => setHours(value)) }} />
      <select aria-label="日志级别" value={level} onChange={event => reset(() => setLevel(event.target.value))}>{LEVELS.map(value => <option key={value} value={value}>{value || '全部级别'}</option>)}</select>
      <input aria-label="搜索日志" value={input} onChange={event => setInput(event.target.value)} placeholder="按原文搜索" />
      <button className="btn btn-sm btn-primary" type="submit">搜索</button>
      <button className="btn btn-sm btn-secondary" type="button" onClick={() => { setLogs(null); if (before == null) void load(); else setBefore(null) }}>刷新</button>
    </form>
    {(selection.component || selection.event || selection.context || selection.id) && <div className="analytics-controls"><span>下钻筛选：{[selection.component, selection.event, selection.context, selection.id ? `日志 #${selection.id}` : ''].filter(Boolean).join(' · ')}</span><button className="btn btn-sm btn-secondary" onClick={() => reset(() => setSelection({}))}>清除下钻筛选</button></div>}
    {error && <div className="connections-error" role="alert">{error}</div>}
    {loading && !logs ? <div className="connections-empty inline">日志加载中…</div> : logs?.entries.length ? <div className="analytics-logs">{logs.entries.map(entry => <article key={entry.id}><header><time>{new Date(entry.timestamp).toLocaleString()}</time><b>{entry.level}</b><span>{entry.component} · {entry.event} · {entry.stream}</span>{entry.context && <button className="analytics-link" onClick={() => reset(() => { setLevel(''); setQuery(''); setInput(''); setSelection({ context: entry.context }) })}>同上下文</button>}</header><pre>{entry.message}</pre></article>)}</div> : <div className="connections-empty inline">暂无匹配日志</div>}
    <div className="analytics-log-footer"><span>日志为运行记录，不包含网络载荷。</span>{logs?.next_before != null && <button className="btn btn-sm btn-secondary" disabled={loading} onClick={() => { setLogs(null); setBefore(logs.next_before) }}>更早日志</button>}</div>
  </div>
}
