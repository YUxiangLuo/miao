import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
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
const chartTime = (value: number) => new Date(value).toLocaleString([], { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })
const SECTIONS = [['overview', '活动概览'], ['dns', 'DNS 行为'], ['routes', '分流与出口'], ['issues', '错误与告警'], ['traffic', '流量采样'], ['coverage', '日志覆盖']] as const
export type AnalysisSection = (typeof SECTIONS)[number][0]
const EVENT_LABELS: Record<string, string> = {
  'dns.complete': 'DNS 完成', 'dns.query': 'DNS 交换开始', 'dns.lookup': '域名解析开始',
  'dns.cached_record': '缓存答案记录', 'dns.optimistic_record': '乐观缓存记录',
  'dns.answer': 'DNS 答案记录', 'dns.refresh': 'DNS 刷新记录',
  'route.match': '规则匹配', 'route.sniff': '协议嗅探',
  'inbound.accept': '入站接收', 'outbound.connect': '出站发起',
  other: '正文未分类', unparsed: '前缀未识别',
}

/** All charts share bucket boundaries and a hover cursor, like a metrics workspace. */
function Timeline({ title, values, timestamps, unit, active, onActive, tone = 'info' }: {
  title: string; values: Array<number | null>; timestamps: number[]; unit: string; active: number | null;
  onActive: (value: number | null) => void; tone?: string;
}) {
  const peak = Math.max(0, ...values.filter((v): v is number => v != null))
  const max = peak > 0 ? peak : 1
  const x = (index: number) => values.length === 1 ? 500 : index * 1000 / Math.max(1, values.length - 1)
  const y = (value: number) => 110 - value / max * 100
  const point = (value: number, index: number) => `${x(index)},${y(value)}`
  const number = (value: number) => value.toLocaleString([], unit === 'ms' ? { maximumSignificantDigits: 4 } : undefined)
  const points = values.map((value, index) => value == null ? '' : point(value, index)).join(' ')
  const line = values.map((value, index) => value == null ? '' : `${index === 0 || values[index - 1] == null ? 'M' : 'L'}${point(value, index)}`).join(' ')
  return <section className={`analytics-section analytics-timeline tone-${tone}`}>
    <div className="analytics-section-head"><h4>{title}</h4><span>{active == null ? `峰值 ${values.some(v => v != null) ? number(peak) : '—'} ${unit}` : `${values[active] == null ? '—' : number(values[active])} ${unit} · ${chartTime(timestamps[active])}`}</span></div>
    <svg viewBox="0 0 1000 120" preserveAspectRatio="none" role="img" aria-label={`${title}，单位 ${unit}`} onPointerLeave={() => onActive(null)} onPointerMove={event => {
      const rect = event.currentTarget.getBoundingClientRect()
      if (!values.length) return
      onActive(Math.max(0, Math.min(values.length - 1, Math.round((event.clientX - rect.left) / rect.width * (values.length - 1)))))
    }}>
      <path className="chart-grid" d="M0 10H1000 M0 60H1000 M0 110H1000" />
      {values.length > 1 && !values.includes(null) && <polygon className="chart-fill" points={`0,110 ${points} 1000,110`} />}
      <path className="chart-line" d={line} />
      {values.map((value, index) => value != null && values[index - 1] == null && values[index + 1] == null ? <circle key={timestamps[index]} className="chart-point" cx={x(index)} cy={y(value)} r="3" /> : null)}
      {active != null && <line className="chart-cursor" x1={x(active)} x2={x(active)} y1="0" y2="120" />}
    </svg>
    <footer><span>{timestamps.length ? chartTime(timestamps[0]) : '暂无记录'}</span><span>每格 5 分钟</span><span>{timestamps.length ? chartTime(timestamps[timestamps.length - 1]) : ''}</span></footer>
  </section>
}

function Facets({ title, facets, dimension, onExplore }: { title: string; facets: LogFacet[]; dimension: string; onExplore: (selection: LogSelection) => void }) {
  const rows = facets.filter(row => row.dimension === dimension)
  const max = Math.max(1, ...rows.map(row => row.count))
  const filter = dimension === 'inbound' || dimension === 'outbound' ? 'component' : dimension
  return <section className="analytics-section"><h4>{title}<small>日志记录数 · Top 20</small></h4>
    {rows.length ? <ol className="analytics-facets">{rows.map(row => <li key={row.name}>
      <button onClick={() => onExplore(dimension === 'route' ? { q: row.name, event: 'route.match' } : { [filter]: row.name })} title={`查看对应原始日志：${row.name}`}>
        <span className="facet-bar" style={{ width: `${row.count / max * 100}%` }} /><span>{dimension === 'event' ? EVENT_LABELS[row.name] ?? row.name : row.name}</span><strong>{row.count.toLocaleString()}</strong>
      </button></li>)}</ol> : <p className="analytics-empty">该时段暂无记录</p>}
  </section>
}

export function AnalyticsSummaryPane({ visible, onExplore, hours: selectedHours, onHoursChange, section: selectedSection, onSectionChange }: {
  visible: boolean; onExplore: (selection: LogSelection) => void;
  hours?: Hours; onHoursChange?: (hours: Hours) => void;
  section?: AnalysisSection; onSectionChange?: (section: AnalysisSection) => void;
}) {
  const pane = useRef<HTMLDivElement>(null)
  const [localHours, setLocalHours] = useState<Hours>(24)
  const hours = selectedHours ?? localHours
  const setHours = onHoursChange ?? setLocalHours
  const [summary, setSummary] = useState<AnalyticsSummary | null>(null)
  const [dimension, setDimension] = useState<(typeof DIMENSIONS)[number][0]>('domain')
  const [localSection, setLocalSection] = useState<AnalysisSection>('overview')
  const section = selectedSection ?? localSection
  const setSection = onSectionChange ?? setLocalSection
  const [dnsDimension, setDnsDimension] = useState('domain')
  const [dnsRanking, setDnsRanking] = useState('queries')
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
  const dnsGroups = summary ? dnsRanking === 'slow' ? summary.dns.slow_domains : dnsRanking === 'failures' ? summary.dns.failed_domains : summary.dns.groups.filter(row => row.dimension === dnsDimension) : []
  const warningCount = summary?.activity.facets.find(row => row.dimension === 'level' && row.name === 'warn')?.count ?? 0
  return <div className="analytics-workspace">
    <aside className="analytics-nav"><strong>网络活动分析</strong><small>sing-box · 最近 24 小时</small>{SECTIONS.map(([key, label]) => <button key={key} aria-current={section === key ? 'page' : undefined} onClick={() => { setSection(key); setActive(null); if (pane.current) pane.current.scrollTop = 0 }}>{label}</button>)}<p>从时间趋势到事件原文<br />点击排行即可下钻</p></aside>
    <div className="analytics-pane" ref={pane}>
    <div className="analytics-controls"><div><h3>{SECTIONS.find(([key]) => key === section)?.[1]}</h3><small>{section === 'traffic' ? 'Clash API 采样' : 'sing-box 日志'} · 5 分钟分桶 · 每 10 秒刷新</small></div><HoursPicker value={hours} onChange={setHours} />
      <button className="btn btn-sm btn-secondary" onClick={() => void load()} disabled={loading}><RefreshCw size={ICON.sm} />刷新</button></div>
    {error && <div className="connections-error" role="alert">{error}</div>}
    {loading && !summary ? <div className="connections-empty inline">分析加载中…</div> : summary ? <>
      {summary.collection_error && <div className="connections-error">采集提示：{summary.collection_error}</div>}
      <p className="analytics-note">查询最近 {hours} 小时；{timestamps.length ? <>图表范围 {chartTime(timestamps[0])} — {chartTime(summary.until)}，从首条日志或采样所在分桶开始。首尾分桶可能不足 5 分钟。</> : '该时段尚无日志或采样记录。'}</p>
      {section === 'overview' && <>
        <div className="analytics-metrics">
          <article><span>日志活动</span><strong>{summary.log_count.toLocaleString()}</strong><small>已分类事件 {summary.activity.classified_count.toLocaleString()} 条</small></article>
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
          <div className="analytics-grid">
            <Timeline {...chartProps} title="DNS 活动趋势" values={timestamps.map(t => dnsBuckets.get(t)?.queries ?? 0)} unit="次" />
            <Timeline {...chartProps} title="DNS 非成功趋势" values={timestamps.map(t => dnsBuckets.get(t)?.failures ?? 0)} unit="次" tone="danger" />
            <Timeline {...chartProps} title="DNS P95 延迟" values={timestamps.map(t => dnsBuckets.get(t)?.p95_ms ?? null)} unit="ms" tone="accent" />
            <Timeline {...chartProps} title="DNS 最慢耗时趋势" values={timestamps.map(t => dnsBuckets.get(t)?.max_ms ?? null)} unit="ms" tone="accent" />
          </div>
          <section className="analytics-section"><h4>最慢 DNS 事件<small>单次真实耗时 · Top 20 · 含失败</small></h4><div className="analytics-table-wrap"><table><thead><tr><th>域名 / 时间</th><th>类型</th><th>耗时</th><th>结果</th><th>上游</th><th>事件</th></tr></thead><tbody>{summary.dns.slowest.map(row => <tr key={row.log_id}>
            <td><button className="analytics-link" onClick={() => explore({ q: row.domain, component: 'dns' })}>{row.domain || '（空查询）'}</button><small>{new Date(row.timestamp).toLocaleString()}</small></td><td>{row.query_type}</td><td>{duration(row.duration_ms)}</td><td title={row.error ?? ''}>{row.outcome}{row.rcode != null ? ` · ${row.rcode}` : ''}</td><td>{row.transport ?? '未记录'}</td><td><button className="analytics-link" onClick={() => explore({ id: row.log_id })}>原文</button>{row.context && <button className="analytics-link" onClick={() => explore({ context: row.context })}>上下文</button>}</td>
          </tr>)}</tbody></table></div></section>
          <section className="analytics-section"><div className="analytics-section-head"><h4>DNS 分布与排行<small>全时段数据聚合 · Top 20 · 慢域名包含失败</small></h4>
            <select aria-label="DNS 排行依据" value={dnsRanking} onChange={event => setDnsRanking(event.target.value)}><option value="queries">按次数</option><option value="slow">慢域名 · 按最大耗时</option><option value="failures">失败域名 · 按非成功次数</option></select>
            {dnsRanking === 'queries' && <select aria-label="DNS 分析维度" value={dnsDimension} onChange={event => setDnsDimension(event.target.value)}>{[['domain', '域名'], ['query_type', '查询类型'], ['transport', '上游'], ['outcome', '结果']].map(([key, label]) => <option key={key} value={key}>{label}</option>)}</select>}
          </div>{dnsGroups.length ? <div className="analytics-table-wrap"><table><thead><tr><th>维度</th><th>次数</th><th>非成功</th><th>非成功占比</th><th>P95</th><th>最慢</th></tr></thead><tbody>{dnsGroups.map(row => <tr key={row.name}><td>{row.dimension === 'domain' ? <button className="analytics-link" title="在 DNS 原文中搜索此域名" onClick={() => explore({ component: 'dns', q: row.name })}>{row.name || '（空查询）'}</button> : row.name}</td><td>{row.queries}</td><td>{row.failures}</td><td>{row.queries ? `${(row.failures / row.queries * 100).toFixed(1)}%` : '—'}</td><td>{duration(row.p95_ms)}</td><td>{duration(row.max_ms)}</td></tr>)}</tbody></table></div> : <p className="analytics-empty">该排行暂无记录</p>}</section>
        </>}
        <p className="analytics-note">耗时来自内核单调时钟，包含路由、缓存及等待时间。LOOKUP 是一次完整解析，可能包含 A/AAAA，不能当作单个上游请求；未完成的查询不在完成事件中。缓存日志可能逐条记录答案，不能据此计算缓存命中率。</p>
      </>}
      {section === 'routes' && <><div className="analytics-grid">
        <Facets title="入站组件活动" facets={summary.activity.facets} dimension="inbound" onExplore={explore} />
        <Facets title="出口组件活动" facets={summary.activity.facets} dimension="outbound" onExplore={explore} />
        <Facets title="规则命中记录" facets={summary.activity.facets} dimension="route" onExplore={explore} />
        <Facets title="路由与连接事件" facets={summary.activity.facets.filter(row => /^(route|inbound|outbound)\./.test(row.name))} dimension="event" onExplore={explore} />
      </div><p className="analytics-note">组件活动包含该组件各级别日志，不是成功连接数。规则匹配可能包含 DNS 与连接路由、中间判断，不等同最终连接数；出站发起也不表示连接成功。</p></>}
      {section === 'issues' && <>
        <div className="analytics-metrics">
          <article><span>错误日志</span><strong>{summary.error_count.toLocaleString()}</strong><small>ERROR / FATAL / PANIC</small></article>
          <article><span>告警日志</span><strong>{warningCount.toLocaleString()}</strong><small>WARN · 不计入错误总量</small></article>
          <article><span>DNS 非成功完成</span><strong>{summary.dns.failures.toLocaleString()}</strong><small>按完成结果统计，与日志级别独立</small></article>
        </div>
        <div className="analytics-grid"><Timeline {...chartProps} title="错误级别日志趋势" values={summary.timeline.map(b => b.error_count)} unit="条" tone="danger" /><Facets title="错误与告警级别" facets={summary.activity.facets.filter(row => ['warn', 'error', 'fatal', 'panic'].includes(row.name))} dimension="level" onExplore={explore} /></div>
        <section className="analytics-section"><h4>错误与告警来源<small>组件 × 级别 · 按日志数 Top 20 · 上下文数在各组内去重，不可跨组相加</small></h4>
          {summary.activity.issues.length ? <div className="analytics-table-wrap"><table><thead><tr><th>组件</th><th>级别</th><th>日志数</th><th>上下文数</th><th>最近出现</th><th>记录</th></tr></thead><tbody>{summary.activity.issues.map(row => <tr key={`${row.component}/${row.level}`}><td>{row.component || '未识别组件'}</td><td>{row.level.toUpperCase()}</td><td>{row.count.toLocaleString()}</td><td>{row.contexts.toLocaleString()}</td><td>{new Date(row.latest_at).toLocaleString()}</td><td><button className="analytics-link" aria-label={`查看 ${row.component || '未识别组件'} ${row.level} 日志`} onClick={() => explore({ level: row.level, ...(row.component ? { component: row.component } : { event: 'unparsed' }) })}>查看日志</button></td></tr>)}</tbody></table></div> : <p className="analytics-empty">该时段没有错误或告警日志</p>}
        </section><p className="analytics-note">这是日志来源分布，不是根因诊断或连接失败率。一条连接可能产生多条错误；DNS 的非成功完成也不能与 ERROR 日志相加。</p>
      </>}
      {section === 'coverage' && <>
        <div className="analytics-metrics">
          <article><span>已分类事件</span><strong>{summary.activity.classified_count.toLocaleString()}</strong><small>识别了正文中的事件类型</small></article>
          <article><span>仅识别前缀</span><strong>{(summary.activity.parsed_count - summary.activity.classified_count).toLocaleString()}</strong><small>正文保留，但尚未分类</small><button className="analytics-link" onClick={() => explore({ event: 'other' })}>查看未分类正文</button></article>
          <article><span>未识别前缀</span><strong>{(summary.log_count - summary.activity.parsed_count).toLocaleString()}</strong><small>原文照常保存，可检索</small><button className="analytics-link" onClick={() => explore({ event: 'unparsed' })}>查看未识别日志</button></article>
        </div>
        <div className="analytics-grid"><Facets title="日志级别" facets={summary.activity.facets} dimension="level" onExplore={explore} /><Facets title="事件分类覆盖" facets={summary.activity.facets} dimension="event" onExplore={explore} /></div>
        <p className="analytics-note">以上三类互斥，合计为保存的日志条数；识别前缀不代表理解正文。统计只覆盖已收到的日志，不能据此证明没有丢失记录。缓存答案条数不是缓存命中次数。</p>
      </>}
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
