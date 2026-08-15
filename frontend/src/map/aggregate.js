import { cityKey, cityLabel, serviceForDestination } from './serviceLabel.js'

export const ACTIVE_SPEED_THRESHOLD = 1024

export function isFlowActive(flow, threshold = ACTIVE_SPEED_THRESHOLD) {
  const download = Number(flow?.download_speed ?? flow?.downloadSpeed ?? 0)
  const upload = Number(flow?.upload_speed ?? flow?.uploadSpeed ?? 0)
  return download + upload >= threshold
}

export function flowSearchText(flow) {
  const service = serviceForDestination(flow.destination?.domain, flow.destination?.ip)
  return [
    service.label,
    service.id,
    flow.destination?.domain,
    flow.destination?.ip,
    flow.destination?.geo?.city,
    flow.destination?.geo?.country,
    flow.proxy?.name,
    flow.proxy?.server,
    flow.proxy?.geo?.city,
    flow.rule,
    flow.network,
    flow.port,
  ].filter(Boolean).join(' ').toLowerCase()
}

export function filterFlows(flows, {
  route = 'all',
  protocol = 'all',
  active = 'all',
  query = '',
} = {}) {
  const needle = String(query || '').trim().toLowerCase()
  return (flows || []).filter((flow) => {
    if (route === 'direct' && flow.proxy) return false
    if (route === 'proxy' && !flow.proxy) return false
    const network = String(flow.network || 'tcp').toLowerCase()
    if (protocol !== 'all' && network !== protocol) return false
    if (active === 'active' && !isFlowActive(flow)) return false
    if (active === 'idle' && isFlowActive(flow)) return false
    if (needle && !flowSearchText(flow).includes(needle)) return false
    return true
  })
}

export function aggregateDestinationGroups(flows) {
  const groups = new Map()

  for (const flow of flows || []) {
    const dest = flow.destination || {}
    const service = serviceForDestination(dest.domain, dest.ip)
    const city = cityKey(dest.geo)
    const id = `${service.id}::${city}`
    let group = groups.get(id)
    if (!group) {
      group = {
        id,
        service,
        city: cityLabel(dest.geo),
        country: dest.geo?.country || dest.geo?.country_code || '',
        geo: dest.geo || null,
        located: dest.geo?.latitude != null && dest.geo?.longitude != null,
        flows: [],
        downloadSpeed: 0,
        uploadSpeed: 0,
        downloadTotal: 0,
        uploadTotal: 0,
      }
      groups.set(id, group)
    }
    group.flows.push(flow)
    group.downloadSpeed += Number(flow.download_speed || 0)
    group.uploadSpeed += Number(flow.upload_speed || 0)
    group.downloadTotal += Number(flow.download_total || 0)
    group.uploadTotal += Number(flow.upload_total || 0)
  }

  return [...groups.values()].map((group) => ({
    ...group,
    count: group.flows.length,
    active: group.flows.some((flow) => isFlowActive(flow)),
  }))
}

export function aggregateProxyCities(proxies, flows = [], delays = {}) {
  const groups = new Map()
  const activeNames = new Set(
    (flows || []).map((flow) => flow.proxy?.name).filter(Boolean),
  )

  for (const proxy of proxies || []) {
    const key = cityKey(proxy.geo)
    const id = `proxy::${key}`
    let group = groups.get(id)
    if (!group) {
      group = {
        id,
        city: cityLabel(proxy.geo, proxy.name),
        country: proxy.geo?.country || proxy.geo?.country_code || '',
        geo: proxy.geo || null,
        located: proxy.geo?.latitude != null && proxy.geo?.longitude != null,
        nodes: [],
      }
      groups.set(id, group)
    }
    const delay = delays[proxy.name]
    group.nodes.push({
      ...proxy,
      delay: typeof delay === 'number' ? delay : proxy.delay,
    })
  }

  return [...groups.values()].map((group) => {
    const measured = group.nodes
      .map((node) => node.delay)
      .filter((delay) => typeof delay === 'number' && delay > 0)
    return {
      ...group,
      count: group.nodes.length,
      bestDelay: measured.length ? Math.min(...measured) : undefined,
      activeNode: group.nodes.find((node) => activeNames.has(node.name))?.name || '',
    }
  })
}

export function aggregatePaths(destinationGroups) {
  const paths = new Map()

  for (const dest of destinationGroups || []) {
    for (const flow of dest.flows) {
      const network = String(flow.network || 'tcp').toLowerCase() === 'udp' ? 'udp' : 'tcp'
      const via = flow.proxy ? cityKey(flow.proxy.geo) : 'direct'
      const id = `${dest.id}::${via}::${network}`
      let path = paths.get(id)
      if (!path) {
        path = {
          id,
          destId: dest.id,
          destGeo: dest.geo,
          proxyGeo: flow.proxy?.geo || null,
          proxyName: flow.proxy?.name || null,
          network,
          direct: !flow.proxy,
          flows: [],
          downloadSpeed: 0,
          uploadSpeed: 0,
        }
        paths.set(id, path)
      }
      path.flows.push(flow)
      path.downloadSpeed += Number(flow.download_speed || 0)
      path.uploadSpeed += Number(flow.upload_speed || 0)
    }
  }

  return [...paths.values()].map((path) => ({
    ...path,
    count: path.flows.length,
    active: isFlowActive(path),
  }))
}

export function routeCounts(flows) {
  return (flows || []).reduce((counts, flow) => {
    counts.all += 1
    if (flow.proxy) counts.proxy += 1
    else counts.direct += 1
    if (String(flow.network || '').toLowerCase() === 'udp') counts.udp += 1
    else counts.tcp += 1
    if (isFlowActive(flow)) counts.active += 1
    else counts.idle += 1
    return counts
  }, { all: 0, proxy: 0, direct: 0, tcp: 0, udp: 0, active: 0, idle: 0 })
}
