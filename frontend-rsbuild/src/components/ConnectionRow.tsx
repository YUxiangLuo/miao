import { Show, createMemo } from 'solid-js'
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
import { cleanNodeDisplay } from './SelectPopover'
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

function formatCleanTarget(raw: string): string {
  const cleaned = cleanNodeDisplay(raw)
  return cleaned.code ? `${cleaned.code} · ${cleaned.name}` : cleaned.name
}

function formatStudioRuleText(rawRule: string): string {
  const base = humanizeClashRule(rawRule) || displayRuleText(rawRule)
  if (!base || base === '-') return '-'

  // Normalize sing-box access-port inbound rules (`inbound access-port:ap-... → ...` or `inbound=access-port:... => route(...)`)
  const inboundPortArrow = /^inbound[\s=]+access-port:[A-Za-z0-9_-]+\s*(?:→|=>)\s*(?:route\(([^)]+)\)|(.+))$/i.exec(base)
  if (inboundPortArrow) {
    const target = (inboundPortArrow[1] || inboundPortArrow[2] || '').trim()
    return target ? `独立端口 → ${formatCleanTarget(target)}` : '独立端口'
  }

  const arrowIdx = base.lastIndexOf(' → ')
  if (arrowIdx > 0) {
    const lhsRaw = base.slice(0, arrowIdx).replace(/^inbound\s+/i, '入站 ')
    const rhsRaw = base.slice(arrowIdx + 3)
    return `${formatCleanTarget(lhsRaw)} → ${formatCleanTarget(rhsRaw)}`
  }

  const routeFallback = /^(.+?)\s*=>\s*route\(([^)]+)\)$/i.exec(base)
  if (routeFallback) {
    const lhsRaw = routeFallback[1].trim().replace(/^inbound=/i, '入站 ')
    return `${formatCleanTarget(lhsRaw)} → ${formatCleanTarget(routeFallback[2])}`
  }

  return formatCleanTarget(base)
}

export function ConnectionRow(props: ConnectionRowProps) {
  const domain = createMemo(() => connectionDomain(props.connection))
  const outbound = createMemo(() => connectionOutbound(props.connection))
  const direct = createMemo(() => isDirectOutbound(outbound()))
  const outboundDisplay = createMemo(() => cleanNodeDisplay(outbound()))

  const subtitle = createMemo(() => {
    const meta = props.connection.metadata || {}
    const sourceIp = meta.sourceIP ? (props.seatLabels?.[meta.sourceIP] || meta.sourceIP) : null
    const portId = connectionAccessPortId(props.connection)
    const portTag = portId ? `端口: ${props.portLabels?.[portId] || portId}` : null
    const rule = connectionRule(props.connection)
    const ruleText = formatStudioRuleText(rule)
    const processName = processNameOf(meta.processPath)
    const network = [
      meta.network ? String(meta.network).toUpperCase() : '',
      meta.destinationPort || '',
    ].filter(Boolean).join('/')
    const duration = connectionDuration(props.connection)
    return [sourceIp, portTag, ruleText !== '-' ? ruleText : null, processName, network, duration !== '--' ? duration : null]
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
        <span
          class={classNames('badge', 'connection-outbound-chip', direct() && 'direct')}
          title={direct() ? outbound() : outboundDisplay().name}
        >
          <Show when={!direct() && outboundDisplay().code}>
            {(code) => <span class="connection-outbound-code">{code()}</span>}
          </Show>
          <span class="connection-outbound-name">
            {direct() ? outbound() : outboundDisplay().name}
          </span>
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
