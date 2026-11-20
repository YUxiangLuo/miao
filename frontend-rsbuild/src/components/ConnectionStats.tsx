import { For, Show, createMemo } from 'solid-js'
import { ICON, PATH_ICON_CELL } from '../tokens'
import { formatBytes, formatSpeed } from '../utils'
import { useElementWidth } from '../hooks/useElementWidth'
import type { EnrichedConnection } from '../types/clash'
import { domainsForPath, splitConnectionStats, uniqueIconDomains, type PathStats } from './connectionFilters'
import { AnimatedValue, SiteMark } from './connectionUi'
import { foldIcons, MAX_ICONS } from './iconFold'
import { ArrowDownIcon, ArrowUpIcon } from './icons'

function PathCard(props: {
  tone: 'info' | 'success'
  label: string
  stats: PathStats
  domains: string[]
}) {
  const unique = createMemo(() => uniqueIconDomains(props.domains))
  const { ref, width } = useElementWidth<HTMLDivElement>()
  const plan = createMemo(() => {
    const w = width()
    const fit = Number.isFinite(w) ? Math.max(1, Math.floor(w / PATH_ICON_CELL)) : MAX_ICONS
    return foldIcons(unique().length, fit)
  })
  const shown = createMemo(() => unique().slice(0, plan().shown))

  return (
    <div class="path-card">
      <div class="path-id">
        <span class={`badge ${props.tone}`}>{props.label}</span>
        <span class="path-count num">{props.stats.count} 条链接</span>
      </div>
      <div class="path-col speed">
        <small class="tone-download">
          <ArrowDownIcon size={ICON.sm} />
          <AnimatedValue value={formatSpeed(props.stats.downloadSpeed)} />
        </small>
        <small class="tone-upload">
          <ArrowUpIcon size={ICON.sm} />
          <AnimatedValue value={formatSpeed(props.stats.uploadSpeed)} />
        </small>
      </div>
      <div class="path-col total">
        <small class="tone-download">
          <ArrowDownIcon size={ICON.xs} />
          <AnimatedValue value={formatBytes(props.stats.download)} />
        </small>
        <small class="tone-upload">
          <ArrowUpIcon size={ICON.xs} />
          <AnimatedValue value={formatBytes(props.stats.upload)} />
        </small>
      </div>
      <div ref={ref} class="path-icons">
        <For each={shown()}>
          {(domain) => <SiteMark domain={domain} />}
        </For>
        <Show when={plan().more > 0}>
          <span class="path-icons-more num">+{plan().more}</span>
        </Show>
      </div>
    </div>
  )
}

export function ConnectionStats(props: { connections: EnrichedConnection[] }) {
  const stats = createMemo(() => splitConnectionStats(props.connections))
  const proxyDomains = createMemo(() => domainsForPath(props.connections, false))
  const directDomains = createMemo(() => domainsForPath(props.connections, true))

  return (
    <div class="path-stats">
      <PathCard tone="info" label="代理" stats={stats().proxy} domains={proxyDomains()} />
      <PathCard tone="success" label="直连" stats={stats().direct} domains={directDomains()} />
    </div>
  )
}
