import { createMemo } from 'solid-js'
import { ICON } from '../tokens'
import { classNames, formatBytes, formatSpeed } from '../utils'
import type { EnrichedConnection } from '../types/clash'
import {
  connectionOutbound,
  connectionRule,
  connectionSpeed,
  connectionDuration,
  humanizeClashRule,
  displayRuleText,
  isDirectOutbound,
  processNameOf,
} from './connectionFilters'
import { connectionAccessPortId } from '../domain/accessPorts'
import { connectionDomain } from './siteIcons'
import { AnimatedValue, SiteMark } from './connectionUi'
import { ArrowDownIcon, ArrowUpIcon } from './icons'

export interface ConnectionRowProps {
  connection: EnrichedConnection
  position?: number
  total?: number
  seatLabels?: Record<string, string>
  portLabels?: Record<string, string>
}

export function ConnectionRow(props: ConnectionRowProps) {
  const domain = createMemo(() => connectionDomain(props.connection))
  const outbound = createMemo(() => connectionOutbound(props.connection))
  const subtitle = createMemo(() => {
    const meta = props.connection.metadata || {}
    const sourceIp = meta.sourceIP ? (props.seatLabels?.[meta.sourceIP] || meta.sourceIP) : null
    const portId = connectionAccessPortId(props.connection)
    const portTag = portId ? `专属端口: ${props.portLabels?.[portId] || portId}` : null
    const rule = connectionRule(props.connection)
    const ruleText = humanizeClashRule(rule) || displayRuleText(rule)
    const processName = processNameOf(meta.processPath)
    const network = [
      meta.network ? String(meta.network).toUpperCase() : '',
      meta.destinationPort || '',
    ].filter(Boolean).join('/')
    const duration = connectionDuration(props.connection)
    return [sourceIp, portTag, ruleText, processName, network, duration !== '--' ? duration : null]
      .filter(Boolean)
      .join(' · ')
  })
  const active = createMemo(() => connectionSpeed(props.connection) > 0)

  return (
    <article
      class={classNames('conn-row', active() ? 'active' : 'idle')}
      aria-label={`${domain()} 链接`}
      data-flip-key={props.connection.id}
      role="listitem"
      aria-posinset={props.position}
      aria-setsize={props.total}
    >
      <div class="conn-row-main">
        <SiteMark domain={domain()} />
        <div class="conn-row-id">
          <strong class="conn-row-title" title={domain()}>{domain()}</strong>
          <span class="conn-row-sub" title={subtitle()}>{subtitle()}</span>
        </div>
        <span class={classNames('badge', 'connection-outbound-chip', isDirectOutbound(outbound()) && 'direct')}>
          {outbound()}
        </span>
        <span class="conn-row-speed">
          <small class="tone-download">
            <ArrowDownIcon size={ICON.xs} />
            <AnimatedValue value={formatSpeed(props.connection.downloadSpeed)} />
          </small>
          <small class="tone-upload">
            <ArrowUpIcon size={ICON.xs} />
            <AnimatedValue value={formatSpeed(props.connection.uploadSpeed)} />
          </small>
        </span>
        <span class="conn-row-total">
          <span class="conn-row-total-label">累计</span>
          <small class="tone-download">
            <ArrowDownIcon size={ICON.xs} />
            <AnimatedValue value={formatBytes(props.connection.download)} />
          </small>
          <small class="tone-upload">
            <ArrowUpIcon size={ICON.xs} />
            <AnimatedValue value={formatBytes(props.connection.upload)} />
          </small>
        </span>
      </div>
    </article>
  )
}
