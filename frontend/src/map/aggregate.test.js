import { describe, expect, it } from 'vitest'
import {
  aggregateDestinationGroups,
  aggregatePaths,
  aggregateProxyCities,
  filterFlows,
  isFlowActive,
} from './aggregate.js'

const tokyo = { country: 'Japan', country_code: 'JP', city: 'Tokyo', latitude: 35.6, longitude: 139.7 }
const frankfurt = { country: 'Germany', country_code: 'DE', city: 'Frankfurt', latitude: 50.1, longitude: 8.6 }
const amsterdam = { country: 'Netherlands', country_code: 'NL', city: 'Amsterdam', latitude: 52.3, longitude: 4.8 }

function flow(overrides = {}) {
  return {
    id: '1',
    network: 'tcp',
    upload_speed: 0,
    download_speed: 0,
    upload_total: 10,
    download_total: 20,
    destination: {
      type: 'destination',
      domain: 'youtube.com',
      ip: '142.250.1.1',
      geo: frankfurt,
    },
    proxy: {
      type: 'proxy',
      name: 'Tokyo 01',
      server: 'tokyo.example.com',
      geo: tokyo,
    },
    ...overrides,
  }
}

describe('map aggregation', () => {
  it('treats low throughput as idle', () => {
    expect(isFlowActive(flow({ download_speed: 100 }))).toBe(false)
    expect(isFlowActive(flow({ download_speed: 2048 }))).toBe(true)
  })

  it('filters by route, protocol, activity and search text', () => {
    const flows = [
      flow({ id: 'yt', download_speed: 5000 }),
      flow({
        id: 'gh',
        proxy: undefined,
        destination: { type: 'destination', domain: 'github.com', ip: '20.1.1.1', geo: amsterdam },
        network: 'udp',
      }),
    ]

    expect(filterFlows(flows, { route: 'direct' }).map((item) => item.id)).toEqual(['gh'])
    expect(filterFlows(flows, { protocol: 'udp' }).map((item) => item.id)).toEqual(['gh'])
    expect(filterFlows(flows, { active: 'active' }).map((item) => item.id)).toEqual(['yt'])
    expect(filterFlows(flows, { query: 'amsterdam' }).map((item) => item.id)).toEqual(['gh'])
    expect(filterFlows(flows, { query: 'Tokyo' }).map((item) => item.id)).toEqual(['yt'])
    expect(filterFlows([
      flow({
        id: 'gv',
        destination: { type: 'destination', domain: 'googlevideo.com', ip: '1.1.1.2', geo: frankfurt },
      }),
    ], { query: 'youtube' }).map((item) => item.id)).toEqual(['gv'])
  })

  it('aggregates a site across domains into one city marker', () => {
    const groups = aggregateDestinationGroups([
      flow({
        id: 'a',
        destination: { type: 'destination', domain: 'youtube.com', ip: '1.1.1.1', geo: frankfurt },
      }),
      flow({
        id: 'b',
        destination: { type: 'destination', domain: 'googlevideo.com', ip: '1.1.1.2', geo: frankfurt },
      }),
      flow({
        id: 'c',
        destination: { type: 'destination', domain: 'googlevideo.com', ip: '2.2.2.2', geo: amsterdam },
      }),
    ])

    const frankfurtYoutube = groups.find((group) => group.city === 'Frankfurt')
    const amsterdamYoutube = groups.find((group) => group.city === 'Amsterdam')
    expect(frankfurtYoutube.service.label).toBe('YouTube')
    expect(frankfurtYoutube.count).toBe(2)
    expect(amsterdamYoutube.count).toBe(1)
  })

  it('groups proxy nodes by city and marks the active one', () => {
    const groups = aggregateProxyCities(
      [
        { type: 'proxy', name: 'Tokyo 01', server: 'a.example.com', geo: tokyo },
        { type: 'proxy', name: 'Tokyo 02', server: 'b.example.com', geo: tokyo },
      ],
      [flow()],
      { 'Tokyo 01': 42, 'Tokyo 02': 38 },
    )

    expect(groups).toHaveLength(1)
    expect(groups[0].city).toBe('Tokyo')
    expect(groups[0].count).toBe(2)
    expect(groups[0].bestDelay).toBe(38)
    expect(groups[0].activeNode).toBe('Tokyo 01')
  })

  it('builds one path per destination, exit and protocol', () => {
    const dests = aggregateDestinationGroups([
      flow({ id: 'tcp', network: 'tcp' }),
      flow({ id: 'udp', network: 'udp', destination: { type: 'destination', domain: 'youtube.com', ip: '9.9.9.9', geo: frankfurt } }),
      flow({
        id: 'direct',
        proxy: undefined,
        destination: { type: 'destination', domain: 'github.com', ip: '20.1.1.1', geo: amsterdam },
      }),
    ])
    const paths = aggregatePaths(dests)
    expect(paths.some((path) => path.direct && path.network === 'tcp')).toBe(true)
    expect(paths.filter((path) => !path.direct).map((path) => path.network).sort()).toEqual(['tcp', 'udp'])
  })
})
