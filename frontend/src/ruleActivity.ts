// 规则活跃检测：把自定义规则与 Clash 连接的 rule 文本互相比对
import type { RuleInfo } from './types/api'

/** 活跃检测只消费连接的 rule 文本，调用方传完整 ClashConnection 亦可 */
export interface RuleBearingConnection {
  rule?: string | null
}

const CLASH_ACTION_SEP = ' => '

/** 规则签名：单条件匹配字段 + 值 + 可选出口 */
export interface RuleSignature {
  field: string
  value: string
  target: string | null
}

type ParsedRule = Record<string, unknown>

function firstKnownMatcher(parsed: ParsedRule): Omit<RuleSignature, 'target'> | null {
  const fields = [
    'domain_suffix',
    'domain',
    'domain_keyword',
    'ip_cidr',
    'source_ip_cidr',
    'port',
    'port_range',
    'protocol',
    'process_name',
    'process_path',
    'rule_set',
  ]
  const matched = fields.filter((field) => parsed[field] != null)
  if (matched.length !== 1) return null
  const field = matched[0]
  const rawValue = parsed[field]
  const value = Array.isArray(rawValue)
    ? rawValue.map(String).join(',')
    : String(rawValue)
  return { field, value }
}

function targetFromParsed(parsed: ParsedRule): string | null {
  if (parsed.action === 'reject') return 'reject'
  if (typeof parsed.outbound === 'string' && parsed.outbound) return parsed.outbound
  return null
}

export function signatureFromCustomRule(rule: RuleInfo | null | undefined): RuleSignature | null {
  if (!rule || rule.skipped) return null

  if (rule.field && rule.value != null && String(rule.value) !== '') {
    return {
      field: rule.field,
      value: String(rule.value),
      target: rule.target || null,
    }
  }

  if (!rule.raw) return null
  try {
    const parsed: unknown = JSON.parse(rule.raw)
    if (!parsed || typeof parsed !== 'object') return null
    const matcher = firstKnownMatcher(parsed as ParsedRule)
    if (!matcher) return null
    return { ...matcher, target: targetFromParsed(parsed as ParsedRule) }
  } catch {
    return null
  }
}

export function signatureFromClashRule(text: string | null | undefined): RuleSignature | null {
  const raw = String(text || '').trim()
  if (!raw || raw === 'final' || raw === '-') return null

  const sep = raw.lastIndexOf(CLASH_ACTION_SEP)
  if (sep < 0) return null

  const left = raw.slice(0, sep)
  const action = raw.slice(sep + CLASH_ACTION_SEP.length)
  const eq = left.indexOf('=')
  if (eq < 0) return null

  let target: string | null = null
  if (action === 'reject') {
    target = 'reject'
  } else if (action.startsWith('route(') && action.endsWith(')')) {
    target = action.slice(6, -1)
  } else {
    return null
  }

  return {
    field: left.slice(0, eq),
    value: left.slice(eq + 1),
    target,
  }
}

function signaturesMatch(expected: RuleSignature | null, actual: RuleSignature | null): boolean {
  if (!expected || !actual) return false
  if (expected.field !== actual.field || expected.value !== actual.value) return false
  if (expected.target && actual.target && expected.target !== actual.target) return false
  return true
}

export function isCustomRuleActive(rule: RuleInfo, connections: RuleBearingConnection[]): boolean {
  const expected = signatureFromCustomRule(rule)
  if (!expected || !Array.isArray(connections)) return false
  return connections.some((connection) => (
    signaturesMatch(expected, signatureFromClashRule(connection?.rule))
  ))
}

export function activeRuleIndexes(rules: RuleInfo[], connections: RuleBearingConnection[]): Set<number> {
  const active = new Set<number>()
  if (!Array.isArray(rules) || !Array.isArray(connections) || connections.length === 0) {
    return active
  }
  for (const rule of rules) {
    if (isCustomRuleActive(rule, connections)) active.add(rule.index)
  }
  return active
}
