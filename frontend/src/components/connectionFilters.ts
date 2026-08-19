import type { ClashConnection, ConnectionGroup, EnrichedConnection } from '../types/clash'
import { signatureFromClashRule } from '../ruleActivity'
import { ruleFieldLabel, ruleTargetLabel } from '../ruleFormat'
import { formatUptime } from '../utils'
import { connectionDomain } from './siteIcons'

/** 链接存活时长：「3m 20s」；无法解析 start 时返回 -- */
export function connectionDuration(connection: ClashConnection): string {
  const startedAt = Date.parse(connection.start || '')
  if (!Number.isFinite(startedAt)) return '--'
  return formatUptime(Math.max(0.1, (Date.now() - startedAt) / 1000))
}

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


export interface PathCounts {
  all: number
  proxy: number
  direct: number
}




// 链接统计固定按速度排序（排序选择器已移除）
export function sortConnectionGroups(groups: ConnectionGroup[]): ConnectionGroup[] {
  return [...groups].sort(
    (a, b) => groupSpeed(b) - groupSpeed(a) || a.domain.localeCompare(b.domain),
  )
}

// ---- 链接级工具：面板以链接为单位 ----

/** 链接速率（上下行合计） */
export function connectionSpeed(connection: EnrichedConnection): number {
  return Number(connection.downloadSpeed || 0) + Number(connection.uploadSpeed || 0)
}

/** 直连/代理计数（出口取 chains 首跳） */
export function pathCountsForConnections(connections: EnrichedConnection[]): PathCounts {
  return connections.reduce((counts, connection) => {
    counts.all += 1
    if (isDirectOutbound(connectionOutbound(connection))) counts.direct += 1
    else counts.proxy += 1
    return counts
  }, { all: 0, proxy: 0, direct: 0 })
}

export function filterConnectionsByPath(connections: EnrichedConnection[], path = 'all'): EnrichedConnection[] {
  if (path === 'direct') return connections.filter((c) => isDirectOutbound(connectionOutbound(c)))
  if (path === 'proxy') return connections.filter((c) => !isDirectOutbound(connectionOutbound(c)))
  return connections
}

/** 固定排序：合计速率降序（活跃自然置顶），同速按域名字典序保证稳定 */
export function sortConnections(connections: EnrichedConnection[]): EnrichedConnection[] {
  return [...connections].sort(
    (a, b) => connectionSpeed(b) - connectionSpeed(a)
      || connectionDomain(a).localeCompare(connectionDomain(b)),
  )
}

// ---- 面板汇总：直连 / 代理双通道统计 ----

export interface PathStats {
  count: number
  downloadSpeed: number
  uploadSpeed: number
  download: number
  upload: number
}

export interface SplitStats {
  proxy: PathStats
  direct: PathStats
}

const EMPTY_PATH_STATS: PathStats = { count: 0, downloadSpeed: 0, uploadSpeed: 0, download: 0, upload: 0 }

/**
 * 把存活连接按出口分为直连/代理两通道并各自汇总。
 * 注意：累计量是「当前存活连接」的字节合计——Clash API 的 uploadTotal/downloadTotal
 * 含已关闭连接且无法按通道拆分，故这里不用。
 */
export function splitConnectionStats(connections: EnrichedConnection[]): SplitStats {
  const proxy = { ...EMPTY_PATH_STATS }
  const direct = { ...EMPTY_PATH_STATS }
  for (const connection of connections) {
    const lane = isDirectOutbound(connectionOutbound(connection)) ? direct : proxy
    lane.count += 1
    lane.downloadSpeed += Number(connection.downloadSpeed || 0)
    lane.uploadSpeed += Number(connection.uploadSpeed || 0)
    lane.download += Number(connection.download || 0)
    lane.upload += Number(connection.upload || 0)
  }
  return { proxy, direct }
}

export function connectionRule(connection: ClashConnection): string {
  const rule = connection.rule || '-'
  return connection.rulePayload ? `${rule} : ${connection.rulePayload}` : rule
}

export function connectionOutbound(connection: ClashConnection): string {
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
