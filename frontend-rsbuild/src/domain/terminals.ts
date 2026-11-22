import type { RuleInfo, TerminalAssignMode, TerminalSummary } from '../types/api'
import type { EnrichedConnection } from '../types/clash'

export interface TerminalRow {
  ip: string
  displayName: string
  hasSavedProfile: boolean
  name?: string
  department?: string
  note?: string
  dhcpHostname?: string
  mac?: string
  mode: TerminalAssignMode
  target?: string
  /** 绑定了不存在或前置跳板缺失的节点，后端已自动改写为阻断以防流量意外回退 */
  failClosed: boolean
  online: boolean
  connectionCount: number
  uploadSpeed: number
  downloadSpeed: number
  ruleIndex?: number
}

const IGNORED_SOURCE_IPS = new Set(['127.0.0.1', '0.0.0.0', '::1', '172.18.0.1'])

function ipv4SortKey(ip: string): number {
  const parts = ip.split('.').map((n) => Number(n))
  if (parts.length !== 4 || parts.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    return Number.MAX_SAFE_INTEGER
  }
  return ((parts[0] * 256 + parts[1]) * 256 + parts[2]) * 256 + parts[3]
}

export function normalizeSingleIpv4Cidr(value: string | undefined): string | null {
  if (!value) return null
  const trimmed = value.trim()
  const ip = trimmed.endsWith('/32') ? trimmed.slice(0, -3) : trimmed
  if (!/^\d{1,3}(?:\.\d{1,3}){3}$/.test(ip)) return null
  if (IGNORED_SOURCE_IPS.has(ip)) return null
  return ip
}

export function isSingleSeatRule(rule: RuleInfo): boolean {
  return rule.field === 'source_ip_cidr' && normalizeSingleIpv4Cidr(rule.value) !== null
}

export function buildTerminalRows(input: {
  terminals: TerminalSummary[]
  rules: RuleInfo[]
  connections: EnrichedConnection[]
}): TerminalRow[] {
  const byIp = new Map<string, {
    summary?: TerminalSummary
    rule?: RuleInfo
    conns: EnrichedConnection[]
  }>()

  for (const terminal of input.terminals) {
    if (!terminal.ip || IGNORED_SOURCE_IPS.has(terminal.ip)) continue
    byIp.set(terminal.ip, { summary: terminal, conns: [] })
  }

  for (const rule of input.rules) {
    if (rule.field !== 'source_ip_cidr') continue
    const ip = normalizeSingleIpv4Cidr(rule.value)
    if (!ip) continue
    const entry = byIp.get(ip) ?? { conns: [] }
    if (!entry.rule) entry.rule = rule
    byIp.set(ip, entry)
  }

  for (const conn of input.connections) {
    const sourceIp = conn.metadata?.sourceIP?.trim()
    if (!sourceIp || IGNORED_SOURCE_IPS.has(sourceIp) || sourceIp.includes(':')) continue
    const entry = byIp.get(sourceIp) ?? { conns: [] }
    entry.conns.push(conn)
    byIp.set(sourceIp, entry)
  }

  const rows: TerminalRow[] = []
  for (const [ip, { summary, rule, conns }] of byIp.entries()) {
    let mode: TerminalAssignMode = 'pool'
    let target: string | undefined
    let failClosed = false

    if (rule && rule.target) {
      if (rule.target === 'direct') {
        mode = 'direct'
      } else if (rule.target === 'reject') {
        mode = 'block_overseas'
      } else if (rule.target === 'proxy') {
        mode = 'pool'
      } else {
        mode = 'dedicated'
        target = rule.target
        failClosed = Boolean(rule.skipped)
      }
    }

    let uploadSpeed = 0
    let downloadSpeed = 0
    for (const conn of conns) {
      uploadSpeed += Number(conn.uploadSpeed || 0)
      downloadSpeed += Number(conn.downloadSpeed || 0)
    }

    const displayName = summary?.name
      || summary?.dhcp_hostname
      || ip

    rows.push({
      ip,
      displayName,
      hasSavedProfile: Boolean(summary?.name),
      name: summary?.name,
      department: summary?.department,
      note: summary?.note,
      dhcpHostname: summary?.dhcp_hostname,
      mac: summary?.mac,
      mode,
      target,
      failClosed,
      online: conns.length > 0 || Boolean(summary?.mac),
      connectionCount: conns.length,
      uploadSpeed,
      downloadSpeed,
      ruleIndex: rule?.index,
    })
  }

  return rows.sort((a, b) => (
    Number(b.failClosed) - Number(a.failClosed)
    || (b.downloadSpeed + b.uploadSpeed) - (a.downloadSpeed + a.uploadSpeed)
    || Number(b.connectionCount > 0) - Number(a.connectionCount > 0)
    || ipv4SortKey(a.ip) - ipv4SortKey(b.ip)
  ))
}

export function terminalModeLabel(mode: TerminalAssignMode): string {
  switch (mode) {
    case 'dedicated':
      return '指定节点'
    case 'pool':
      return '默认代理'
    case 'block_overseas':
      return '禁止走代理'
    case 'direct':
      return '全量直连'
  }
}
