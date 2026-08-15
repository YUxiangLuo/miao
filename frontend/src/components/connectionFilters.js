import { connectionDomain } from './siteIcons.js'

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

function connectionRule(connection) {
  const rule = connection.rule || '-'
  return connection.rulePayload ? `${rule} : ${connection.rulePayload}` : rule
}

function connectionOutbound(connection) {
  if (Array.isArray(connection.chains) && connection.chains.length > 0) {
    return connection.chains[0]
  }
  return connection.rule || 'direct'
}

function majorityValue(items, mapper) {
  const counts = new Map()
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

export function groupConnections(connections) {
  const groups = new Map()

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
