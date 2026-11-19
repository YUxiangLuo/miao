import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import {
  collectNodeProtocols,
  collectRuleTargetNodes,
  enrichConnectionsSnapshot,
  isClashProxyGroup,
  mergeDisplayMeasurements,
  useConnections,
} from './useClash'

function deferredResponse() {
  let resolve!: (response: Response) => void
  const promise = new Promise<Response>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function connectionsResponse(id: string): Response {
  return {
    ok: true,
    json: async () => ({
      connections: [{ id, upload: 0, download: 0 }],
      uploadTotal: 0,
      downloadTotal: 0,
    }),
  } as Response
}

afterEach(() => {
  rs.unstubAllGlobals()
})

describe('isClashProxyGroup', () => {
  it('accepts selector and urltest groups from clash api', () => {
    expect(isClashProxyGroup('Selector')).toBe(true)
    expect(isClashProxyGroup('URLTest')).toBe(true)
    expect(isClashProxyGroup('Direct')).toBe(false)
    expect(isClashProxyGroup(undefined)).toBe(false)
  })
})

describe('useConnections', () => {
  it('ignores an older response that finishes after a newer request', async () => {
    const first = deferredResponse()
    const second = deferredResponse()
    rs.stubGlobal('fetch', rs.fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise))
    const { result } = renderHook(() => useConnections({ ready: true }, '/api/clash'))

    let firstCall!: Promise<unknown>
    let secondCall!: Promise<unknown>
    act(() => {
      firstCall = result.current.fetchConnections()
      secondCall = result.current.fetchConnections()
    })

    await act(async () => {
      second.resolve(connectionsResponse('new'))
      await secondCall
    })
    expect(result.current.connectionsInfo.connections[0]?.id).toBe('new')

    await act(async () => {
      first.resolve(connectionsResponse('old'))
      await firstCall
    })
    expect(result.current.connectionsInfo.connections[0]?.id).toBe('new')
  })
})

describe('mergeDisplayMeasurements', () => {
  const t1 = Date.parse('2026-09-22T10:00:00Z')
  const t2 = Date.parse('2026-09-22T10:05:00Z')
  const t3 = Date.parse('2026-09-22T10:10:00Z')

  const proxies = {
    proxy: { type: 'URLTest', all: ['hk-1', 'hk-2', 'hk-zero'], history: [{ time: '2026-09-22T10:10:00Z', delay: 50 }] },
    'hk-1': {
      type: 'Hysteria2',
      history: [
        { time: '2026-09-22T10:00:00Z', delay: 180 },
        { time: '2026-09-22T10:05:00Z', delay: 95 },
      ],
    },
    'hk-2': {
      type: 'VLESS',
      history: [{ time: '2026-09-22T10:05:00Z', delay: 120 }],
    },
    'hk-zero': {
      type: 'Trojan',
      history: [{ time: '2026-09-22T10:05:00Z', delay: 0 }],
    },
  }

  it('ignores URLTest history when the active group is not URLTest', () => {
    const merged = mergeDisplayMeasurements({
      proxies,
      isUrlTestGroup: false,
      delays: { 'hk-1': 210 },
      delayMeasuredAt: { 'hk-1': t1 },
    })
    expect(merged).toEqual({
      delays: { 'hk-1': 210 },
      measuredAt: { 'hk-1': t1 },
    })
  })

  it('merges chronological latest history vs manual probes in URLTest mode', () => {
    const merged = mergeDisplayMeasurements({
      proxies,
      isUrlTestGroup: true,
      // hk-1 has an older manual probe (t1 < t2), hk-2 has a newer manual timeout (t3 > t2)
      delays: { 'hk-1': 210, 'hk-2': -1 },
      delayMeasuredAt: { 'hk-1': t1, 'hk-2': t3 },
    })
    expect(merged.delays).toEqual({
      'hk-1': 95,
      'hk-2': -1,
    })
    expect(merged.measuredAt).toEqual({
      'hk-1': t2,
      'hk-2': t3,
    })
    expect(merged.delays.proxy).toBeUndefined()
    expect(merged.delays['hk-zero']).toBeUndefined()
  })
})

describe('enrichConnectionsSnapshot & Clash node collectors', () => {
  it('clamps sub-second poll intervals and counter resets when computing speeds', () => {
    const first = enrichConnectionsSnapshot(
      {
        uploadTotal: 100,
        downloadTotal: 500,
        connections: [{ id: 'c1', upload: 100, download: 500, start: '', chains: [], rule: 'final', rulePayload: '', metadata: {} }],
      },
      { at: 0, connections: new Map() },
      1_000,
    )
    expect(first.info.connections[0]).toMatchObject({ uploadSpeed: 0, downloadSpeed: 0 })

    // 200ms later (< 1s clamp -> 1s divisor), upload reset from 100 -> 40 (clamped to 0), download +300
    const second = enrichConnectionsSnapshot(
      {
        uploadTotal: 140,
        downloadTotal: 800,
        connections: [{ id: 'c1', upload: 40, download: 800, start: '', chains: [], rule: 'final', rulePayload: '', metadata: {} }],
      },
      first.snapshot,
      1_200,
    )
    expect(second.info.connections[0]).toMatchObject({ uploadSpeed: 0, downloadSpeed: 300 })
  })

  it('collects non-group node protocols and rule target candidates', () => {
    const proxies = {
      proxy: { type: 'Selector', all: ['hk-1', 'direct'] },
      direct: { type: 'Direct' },
      'hk-1': { type: 'Hysteria2' },
    }
    expect(collectNodeProtocols(proxies)).toEqual({ direct: 'Direct', 'hk-1': 'Hysteria2' })
    expect(collectRuleTargetNodes([{ tag: 'manual-1' }, { tag: 'hk-1' }], proxies)).toEqual(['manual-1', 'hk-1'])
  })
})
