export const PATH_FILTERS = [
  { value: 'all', label: '全部' },
  { value: 'proxy', label: '代理' },
  { value: 'direct', label: '直连' },
]

export const SORT_OPTIONS = [
  { value: 'activity', label: '活跃度' },
  { value: 'domain', label: '域名' },
  { value: 'count', label: '连接数' },
]

export function isDirectOutbound(outbound) {
  return String(outbound || '').toLowerCase() === 'direct'
}

export function groupMatchesQuery(group, query) {
  const needle = query.trim().toLowerCase()
  if (!needle) return true
  return [
    group.domain,
    group.rule,
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

export function sortConnectionGroups(groups, sortKey = 'activity') {
  return [...groups].sort((a, b) => {
    if (sortKey === 'domain') return a.domain.localeCompare(b.domain)
    if (sortKey === 'count') return b.count - a.count || a.domain.localeCompare(b.domain)
    return b.downloadSpeed - a.downloadSpeed || a.domain.localeCompare(b.domain)
  })
}
