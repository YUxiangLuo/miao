import type { ClashConnection, ConnectionGroup, EnrichedConnection } from '../types/clash'
import { connectionDomain } from './siteIcons'

export interface PathFilterOption {
  value: string
  label: string
}

export const PATH_FILTERS: PathFilterOption[] = [
  { value: 'all', label: '全部' },
  { value: 'proxy', label: '代理' },
  { value: 'direct', label: '直连' },
]

export function isDirectOutbound(outbound: string): boolean {
  return String(outbound || '').toLowerCase() === 'direct'
}

// sing-box 的兜底规则叫 final,对普通用户不直观,展示时翻译为「兜底规则」
export function displayRuleText(rule: string | null | undefined): string {
  if (!rule || rule === '-') return '-'
  return rule === 'final' ? '兜底规则' : rule
}

export function groupSpeed(group: Pick<ConnectionGroup, 'downloadSpeed' | 'uploadSpeed'>): number {
  return Number(group.downloadSpeed || 0) + Number(group.uploadSpeed || 0)
}

export function groupMatchesQuery(group: ConnectionGroup, query: string): boolean {
  const needle = query.trim().toLowerCase()
  if (!needle) return true
  return [
    group.domain,
    group.rule,
    group.ruleLabel,
    group.outbound,
    ...(Array.isArray(group.connections) ? group.connections.flatMap((connection) => [
      connection.id,
      connection.rule,
      connection.rulePayload,
      ...(Array.isArray(connection.chains) ? connection.chains : []),
    ]) : []),
  ].filter(Boolean).join(' ').toLowerCase().includes(needle)
}

export interface PathCounts {
  all: number
  proxy: number
  direct: number
}

export function pathCountsFor(groups: ConnectionGroup[]): PathCounts {
  return groups.reduce((counts, group) => {
    counts.all += 1
    if (isDirectOutbound(group.outbound)) counts.direct += 1
    else counts.proxy += 1
    return counts
  }, { all: 0, proxy: 0, direct: 0 })
}

export interface ConnectionFilter {
  query?: string
  path?: string
}

export function filterConnectionGroups(groups: ConnectionGroup[], { query = '', path = 'all' }: ConnectionFilter = {}): ConnectionGroup[] {
  return groups.filter((group) => {
    if (path === 'direct' && !isDirectOutbound(group.outbound)) return false
    if (path === 'proxy' && isDirectOutbound(group.outbound)) return false
    return groupMatchesQuery(group, query)
  })
}

// 链接统计固定按速度排序（排序选择器已移除）
export function sortConnectionGroups(groups: ConnectionGroup[]): ConnectionGroup[] {
  return [...groups].sort(
    (a, b) => groupSpeed(b) - groupSpeed(a) || a.domain.localeCompare(b.domain),
  )
}

function connectionRule(connection: ClashConnection): string {
  const rule = connection.rule || '-'
  return connection.rulePayload ? `${rule} : ${connection.rulePayload}` : rule
}

function connectionOutbound(connection: ClashConnection): string {
  if (Array.isArray(connection.chains) && connection.chains.length > 0) {
    return connection.chains[0]
  }
  return connection.rule || 'direct'
}

interface MajorityResult {
  value: string
  extra: number
}

function majorityValue(items: ClashConnection[], mapper: (item: ClashConnection) => string): MajorityResult {
  const counts = new Map<string, number>()
  for (const item of items) {
    const value = mapper(item) || '-'
    counts.set(value, (counts.get(value) || 0) + 1)
  }

  let winner = '-'
  let winnerCount = 0
  for (const [value, count] of counts) {
    if (count > winnerCount) {
      winner = value
      winnerCount = count
    }
  }

  return { value: winner, extra: Math.max(0, counts.size - 1) }
}

export function groupConnections(connections: EnrichedConnection[]): ConnectionGroup[] {
  const groups = new Map<string, Omit<ConnectionGroup, 'count' | 'rule' | 'ruleLabel' | 'extraRules' | 'outbound'>>()

  for (const connection of connections) {
    const domain = connectionDomain(connection)
    const key = domain.toLowerCase()
    const existing = groups.get(key)
    if (existing) {
      existing.connections.push(connection)
      existing.downloadSpeed += Number(connection.downloadSpeed || 0)
      existing.uploadSpeed += Number(connection.uploadSpeed || 0)
      existing.download += Number(connection.download || 0)
      existing.upload += Number(connection.upload || 0)
      continue
    }

    groups.set(key, {
      id: key,
      domain,
      connections: [connection],
      downloadSpeed: Number(connection.downloadSpeed || 0),
      uploadSpeed: Number(connection.uploadSpeed || 0),
      download: Number(connection.download || 0),
      upload: Number(connection.upload || 0),
    })
  }

  return [...groups.values()]
    .map((group) => {
      const rule = majorityValue(group.connections, connectionRule)
      const outbound = majorityValue(group.connections, connectionOutbound)
      return {
        ...group,
        count: group.connections.length,
        rule: rule.value,
        ruleLabel: displayRuleText(rule.value),
        extraRules: rule.extra,
        outbound: outbound.value,
      }
    })
}
