const CLASH_ACTION_SEP = ' => '

function firstKnownMatcher(parsed) {
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

function targetFromParsed(parsed) {
  if (parsed.action === 'reject') return 'reject'
  if (typeof parsed.outbound === 'string' && parsed.outbound) return parsed.outbound
  return null
}

export function signatureFromCustomRule(rule) {
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
    const parsed = JSON.parse(rule.raw)
    if (!parsed || typeof parsed !== 'object') return null
    const matcher = firstKnownMatcher(parsed)
    if (!matcher) return null
    return { ...matcher, target: targetFromParsed(parsed) }
  } catch {
    return null
  }
}

export function signatureFromClashRule(text) {
  const raw = String(text || '').trim()
  if (!raw || raw === 'final' || raw === '-') return null

  const sep = raw.lastIndexOf(CLASH_ACTION_SEP)
  if (sep < 0) return null

  const left = raw.slice(0, sep)
  const action = raw.slice(sep + CLASH_ACTION_SEP.length)
  const eq = left.indexOf('=')
  if (eq < 0) return null

  let target = null
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

export function signaturesMatch(expected, actual) {
  if (!expected || !actual) return false
  if (expected.field !== actual.field || expected.value !== actual.value) return false
  if (expected.target && actual.target && expected.target !== actual.target) return false
  return true
}

export function isCustomRuleActive(rule, connections) {
  const expected = signatureFromCustomRule(rule)
  if (!expected || !Array.isArray(connections)) return false
  return connections.some((connection) => (
    signaturesMatch(expected, signatureFromClashRule(connection?.rule))
  ))
}

export function activeRuleIndexes(rules, connections) {
  const active = new Set()
  if (!Array.isArray(rules) || !Array.isArray(connections) || connections.length === 0) {
    return active
  }
  for (const rule of rules) {
    if (isCustomRuleActive(rule, connections)) active.add(rule.index)
  }
  return active
}
