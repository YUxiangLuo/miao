export const PATH_FILTERS = [
  { value: 'all', label: '全部' },
  { value: 'proxy', label: '代理' },
  { value: 'direct', label: '直连' },
]

export const SORT_OPTIONS = [
  { value: 'speed', label: '速度' },
  { value: 'traffic', label: '流量' },
  { value: 'domain', label: '域名' },
  { value: 'count', label: '链接数' },
]

export function isDirectOutbound(outbound) {
  return String(outbound || '').toLowerCase() === 'direct'
}

// sing-box 的兜底规则叫 final,对普通用户不直观,展示时翻译为「兜底规则」
export function displayRuleText(rule) {
  if (!rule || rule === '-') return '-'
  return rule === 'final' ? '兜底规则' : rule
}

export function groupSpeed(group) {
  return Number(group.downloadSpeed || 0) + Number(group.uploadSpeed || 0)
}

export function groupTraffic(group) {
  return Number(group.download || 0) + Number(group.upload || 0)
}

export function groupMatchesQuery(group, query) {
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

export function pathCountsFor(groups) {
  return groups.reduce((counts, group) => {
    counts.all += 1
    if (isDirectOutbound(group.outbound)) counts.direct += 1
    else counts.proxy += 1
    return counts
  }, { all: 0, proxy: 0, direct: 0 })
}

export function filterConnectionGroups(groups, { query = '', path = 'all' } = {}) {
  return groups.filter((group) => {
    if (path === 'direct' && !isDirectOutbound(group.outbound)) return false
    if (path === 'proxy' && isDirectOutbound(group.outbound)) return false
    return groupMatchesQuery(group, query)
  })
}

export function sortConnectionGroups(groups, sortKey = 'speed') {
  return [...groups].sort((a, b) => {
    if (sortKey === 'domain') return a.domain.localeCompare(b.domain)
    if (sortKey === 'count') return b.count - a.count || a.domain.localeCompare(b.domain)
    if (sortKey === 'speed') return groupSpeed(b) - groupSpeed(a) || a.domain.localeCompare(b.domain)
    return groupTraffic(b) - groupTraffic(a) || a.domain.localeCompare(b.domain)
  })
}
