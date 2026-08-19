import type { ClashConnection, ConnectionGroup, EnrichedConnection, GroupRow } from '../types/clash'
import { signatureFromClashRule } from '../ruleActivity'
import { ruleFieldLabel, ruleTargetLabel } from '../ruleFormat'
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

/** 链接统计的聚合维度选项（ConnectionsToolbar 的维度切换器） */
export const DIMENSION_FILTERS: PathFilterOption[] = [
  { value: 'site', label: '站点' },
  { value: 'process', label: '进程' },
  { value: 'outbound', label: '出口' },
]

export function isDirectOutbound(outbound: string): boolean {
  return String(outbound || '').toLowerCase() === 'direct'
}

// sing-box 的兜底规则叫 final,对普通用户不直观,展示时翻译为「兜底规则」
export function displayRuleText(rule: string | null | undefined): string {
  if (!rule || rule === '-') return '-'
  return rule === 'final' ? '兜底规则' : rule
}

/** 内置规则集的人话标签（ruleFormat 的字段白名单不含 rule_set，那是内部生成的） */
const RULE_SET_LABELS: Record<string, string> = {
  chinasite: '中国站点规则集',
  chinaip: '中国 IP 规则集',
}

/**
 * 把 Clash API 的规则文本翻译成人话：
 * `rule_set=chinasite => route(direct)` → 「中国站点规则集 → 直连」
 * `process_name=curl => route(香港)` → 「进程名 curl → 香港」
 * 无法解析的格式（如 mihomo 的 "RuleSet" 裸类型）返回 null，调用方回退 displayRuleText。
 */
export function humanizeClashRule(text: string | null | undefined): string | null {
  const raw = String(text || '').trim()
  if (!raw || raw === '-') return null
  if (raw === 'final') return '兜底规则'

  const sig = signatureFromClashRule(raw)
  if (!sig) {
    // 无值裸匹配（如 `ip_is_private => route(direct)`）
    const sep = raw.lastIndexOf(' => ')
    if (sep < 0) return null
    const field = raw.slice(0, sep).trim()
    if (!/^[a-z_]+$/.test(field)) return null
    const action = raw.slice(sep + 4)
    const target = actionTargetLabel(action)
    const label = field === 'ip_is_private' ? '私有 IP' : ruleFieldLabel(field)
    return target ? `${label} → ${target}` : label
  }

  let fieldLabel: string
  if (sig.field === 'rule_set') {
    fieldLabel = RULE_SET_LABELS[sig.value] || `规则集 ${sig.value}`
  } else if (sig.field === 'ip_is_private') {
    fieldLabel = '私有 IP'
  } else {
    fieldLabel = `${ruleFieldLabel(sig.field)} ${sig.value}`
  }
  const target = sig.target ? ruleTargetLabel(sig.target) : null
  return target ? `${fieldLabel} → ${target}` : fieldLabel
}

/** `route(direct)` → 「直连」；`reject` → 「拦截」；节点名原样 */
function actionTargetLabel(action: string): string | null {
  if (action === 'reject') return ruleTargetLabel('reject')
  if (action.startsWith('route(') && action.endsWith(')')) {
    return ruleTargetLabel(action.slice(6, -1))
  }
  return null
}

/**
 * 从 sing-box processPath 提取进程名：
 * `/usr/lib/firefox/firefox (alice)` → `firefox`；
 * Windows NT 路径（`\Device\HarddiskVolume3\...`）按反斜杠取段；空值返回 null。
 */
export function processNameOf(processPath: string | null | undefined): string | null {
  if (!processPath) return null
  const base = processPath.split(/[\\/]/).filter(Boolean).pop() || ''
  const name = base.replace(/\s+\([^)]*\)$/, '').trim()
  return name || null
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
        ruleLabel: humanizeClashRule(rule.value) || displayRuleText(rule.value),
        extraRules: rule.extra,
        outbound: outbound.value,
      }
    })
}

// ---- 统一行模型：站点 / 进程 / 出口三个维度都归一为 GroupRow ----

/** 站点视图：groupConnections 的直接投影 */
export function siteGroupRows(groups: ConnectionGroup[]): GroupRow[] {
  return groups.map((group) => ({
    id: group.id,
    title: group.domain,
    subtitle: group.ruleLabel + (group.extraRules > 0 ? ` +${group.extraRules}` : ''),
    mark: group.domain,
    outbound: group.outbound,
    count: group.count,
    downloadSpeed: group.downloadSpeed,
    uploadSpeed: group.uploadSpeed,
    download: group.download,
    upload: group.upload,
    connections: group.connections,
  }))
}

interface GroupAccumulator {
  title: string
  domains: Set<string>
  connections: EnrichedConnection[]
  downloadSpeed: number
  uploadSpeed: number
  download: number
  upload: number
}

function accumulateBy(connections: EnrichedConnection[], keyOf: (c: EnrichedConnection) => string): Map<string, GroupAccumulator> {
  const map = new Map<string, GroupAccumulator>()
  for (const connection of connections) {
    const key = keyOf(connection)
    let acc = map.get(key)
    if (!acc) {
      acc = {
        title: key,
        domains: new Set(),
        connections: [],
        downloadSpeed: 0,
        uploadSpeed: 0,
        download: 0,
        upload: 0,
      }
      map.set(key, acc)
    }
    acc.connections.push(connection)
    acc.domains.add(connectionDomain(connection))
    acc.downloadSpeed += Number(connection.downloadSpeed || 0)
    acc.uploadSpeed += Number(connection.uploadSpeed || 0)
    acc.download += Number(connection.download || 0)
    acc.upload += Number(connection.upload || 0)
  }
  return map
}

function accumulatorToRow(acc: GroupAccumulator): GroupRow {
  const outbound = majorityValue(acc.connections, connectionOutbound)
  return {
    id: acc.title.toLowerCase(),
    title: acc.title,
    subtitle: `${acc.domains.size} 个站点`,
    mark: acc.title,
    outbound: outbound.value,
    count: acc.connections.length,
    downloadSpeed: acc.downloadSpeed,
    uploadSpeed: acc.uploadSpeed,
    download: acc.download,
    upload: acc.upload,
    connections: acc.connections,
  }
}

/** 进程视图：按 processPath 的 basename 聚合（空值归入「未知进程」） */
export function processGroupRows(connections: EnrichedConnection[]): GroupRow[] {
  return [...accumulateBy(
    connections,
    (c) => processNameOf(c.metadata?.processPath) ?? '未知进程',
  ).values()].map(accumulatorToRow)
}

/** 出口视图：按出站（chains 首跳）聚合 */
export function outboundGroupRows(connections: EnrichedConnection[]): GroupRow[] {
  return [...accumulateBy(connections, connectionOutbound).values()].map(accumulatorToRow)
}

export function buildGroupRows(dimension: 'site' | 'process' | 'outbound', connections: EnrichedConnection[]): GroupRow[] {
  if (dimension === 'process') return processGroupRows(connections)
  if (dimension === 'outbound') return outboundGroupRows(connections)
  return siteGroupRows(groupConnections(connections))
}

export function rowMatchesQuery(row: GroupRow, query: string): boolean {
  const needle = query.trim().toLowerCase()
  if (!needle) return true
  return [
    row.title,
    row.subtitle,
    row.outbound,
    ...row.connections.flatMap((connection) => [
      connection.id,
      connection.rule,
      connection.rulePayload,
      connectionDomain(connection),
      ...(Array.isArray(connection.chains) ? connection.chains : []),
    ]),
  ].filter(Boolean).join(' ').toLowerCase().includes(needle)
}

export function pathCountsForRows(rows: GroupRow[]): PathCounts {
  return rows.reduce((counts, row) => {
    counts.all += 1
    if (isDirectOutbound(row.outbound)) counts.direct += 1
    else counts.proxy += 1
    return counts
  }, { all: 0, proxy: 0, direct: 0 })
}

export function filterGroupRows(rows: GroupRow[], { query = '', path = 'all' }: ConnectionFilter = {}): GroupRow[] {
  return rows.filter((row) => {
    if (path === 'direct' && !isDirectOutbound(row.outbound)) return false
    if (path === 'proxy' && isDirectOutbound(row.outbound)) return false
    return rowMatchesQuery(row, query)
  })
}

/** 链接统计固定按合计速度排序；同速按标题字典序保证稳定 */
export function sortGroupRows(rows: GroupRow[]): GroupRow[] {
  return [...rows].sort(
    (a, b) => (b.downloadSpeed + b.uploadSpeed) - (a.downloadSpeed + a.uploadSpeed)
      || a.title.localeCompare(b.title),
  )
}
