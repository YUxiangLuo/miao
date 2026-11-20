import { renderHook } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, expect, it, rs } from '@rstest/core'
import { useStatus, useNodes } from './useResources'
import { useDelays, useProxies } from './useClash'
import {
  isInitialLoadComplete,
  shouldAutoTestCurrentNode,
  shouldEnterOnboarding,
  useAppData,
} from './useAppData'
import { resolveAppScreenMode } from '../App'
import { usePolling } from './usePolling'
import { READ_TIMEOUT_MS } from './request'
import { nodeMock, statusMock } from '../testFixtures'

function deferred() {
  let resolve!: (value: Response) => void
  const promise = new Promise<Response>((done) => { resolve = done })
  return { resolve, promise }
}

function response(data: unknown) {
  return { ok: true, json: async () => data } as Response
}

afterEach(() => {
  rs.useRealTimers()
  rs.unstubAllGlobals()
})

it('retains cached nodes on read failure and clears the error only after successful recovery', async () => {
  const node = nodeMock({ tag: 'cached-node' })
  const fetch = rs.fn()
    .mockResolvedValueOnce(response({ success: true, data: [node] }))
    .mockRejectedValueOnce(new Error('connection lost'))
    .mockResolvedValueOnce(response({ success: true, data: [] }))
  rs.stubGlobal('fetch', fetch)
  const { result } = renderHook(() => useNodes())
  expect(await result.fetchNodes()).toEqual({ status: 'success', data: [node] })
  expect(await result.fetchNodes()).toEqual({ status: 'error', error: 'connection lost' })
  expect(result.nodes()).toEqual([node])
  expect(result.nodesError()).toBe('connection lost')
  await result.fetchNodes()
  expect(result.nodesError()).toBe('')
  expect(result.nodes()).toEqual([])
})

it('does not publish an error from an obsolete list request', async () => {
  const old = deferred()
  rs.stubGlobal('fetch', rs.fn()
    .mockImplementationOnce(() => old.promise)
    .mockResolvedValueOnce(response({ success: true, data: [] })))
  const { result } = renderHook(() => useNodes())
  const previous = result.fetchNodes()
  await result.fetchNodes()
  old.resolve(response({ success: false, message: 'obsolete error' }))
  expect(await previous).toEqual({ status: 'superseded' })
  expect(result.nodesError()).toBe('')
  expect(result.nodesAvailable()).toBe(true)
})

it('uses the request-start revision even through a retained refresh callback', async () => {
  const pending = deferred()
  const fetchMock = rs.fn().mockResolvedValue(response({ success: true, data: [] }))
  rs.stubGlobal('fetch', fetchMock)
  const [revision, setRevision] = createSignal(1)
  const { result } = renderHook(() => useNodes(revision))
  const retained = result.fetchNodes
  await retained()
  setRevision(2)
  flush()
  await result.fetchNodes()
  expect(result.nodesCurrent()).toBe(true)
  await retained()
  expect(result.nodesCurrent()).toBe(true)

  fetchMock.mockImplementationOnce(() => pending.promise)
  const inFlight = retained()
  setRevision(3)
  flush()
  pending.resolve(response({ success: true, data: [] }))
  await inFlight
  expect(result.nodesCurrent()).toBe(false)
  await retained()
  expect(result.nodesCurrent()).toBe(true)
})

it('keeps the latest status when an older response arrives last', async () => {
  const old = deferred()
  const fresh = deferred()
  rs.stubGlobal('fetch', rs.fn().mockImplementationOnce(() => old.promise).mockImplementationOnce(() => fresh.promise))
  const { result } = renderHook(() => useStatus())
  const a = result.fetchStatus()
  const b = result.fetchStatus()
  fresh.resolve(response({ success: true, data: statusMock({ running: true }) }))
  await b
  expect(result.status().ready).toBe(true)
  old.resolve(response({ success: true, data: statusMock({ running: false }) }))
  await a
  expect(result.status().ready).toBe(true)
})

it('discards proxy responses from before the service stopped', async () => {
  const old = deferred()
  rs.stubGlobal('fetch', rs.fn(() => old.promise))
  const [ready, setReady] = createSignal(true)
  const { result } = renderHook(() => useProxies(ready))
  const a = result.fetchProxies()
  setReady(false)
  flush()
  expect(result.proxies()).toEqual({})
  old.resolve(response({ proxies: { proxy: { type: 'Selector', all: ['old-node'], now: 'old-node' } } }))
  await a
  expect(result.primaryGroup()).toBeNull()
})

it('discards measurements after their generation is cleared', async () => {
  const old = deferred()
  rs.stubGlobal('fetch', rs.fn(() => old.promise))
  const { result } = renderHook(() => useDelays())
  const a = result.testDelay('/api/clash', 'old-node')
  result.clearDelays()
  old.resolve(response({ delay: 123 }))
  await a
  expect(result.delays()['old-node']).toBeUndefined()
})

it('gates fresh and retained proxy callbacks while stopped and allows restart', async () => {
  const fetchMock = rs.fn(async () => response({ proxies: { proxy: { type: 'Selector', all: ['node'], now: 'node' } } }))
  rs.stubGlobal('fetch', fetchMock)
  const [ready, setReady] = createSignal(true)
  const { result } = renderHook(() => useProxies(ready))
  const retained = result.fetchProxies
  setReady(false)
  flush()
  await retained()
  await result.fetchProxies()
  expect(result.proxies()).toEqual({})
  expect(fetchMock).not.toHaveBeenCalled()
  setReady(true)
  flush()
  await retained()
  expect(result.primaryGroup()?.now).toBe('node')
})

it.each([false, true])('waits for revised configuration before onboarding (new node: %s)', async (hasNode) => {
  rs.useFakeTimers()
  let revision = 1
  let pending = deferred()
  rs.stubGlobal('fetch', rs.fn(async (input: string) => {
    if (input === '/api/status') return response({ success: true, data: statusMock({ ready: false, data_revision: revision }) })
    if (input === '/api/nodes' && revision === 2) return pending.promise
    return response({ success: true, data: input === '/api/version' ? {} : [] })
  }))
  const { result } = renderHook(() => useAppData())
  flush()
  await rs.advanceTimersByTimeAsync(0)
  flush()
  expect(result.needsOnboarding()).toBe(true)
  revision = 2
  await result.fetchStatus()
  flush()
  expect(result.needsOnboarding()).toBe(false)
  const nodes = hasNode ? [nodeMock({ tag: 'new-node' })] : []
  pending.resolve(response({ success: true, data: nodes }))
  await rs.advanceTimersByTimeAsync(0)
  flush()
  expect(result.needsOnboarding()).toBe(!hasNode)
  pending = deferred()
  await rs.advanceTimersByTimeAsync(30_000)
  flush()
  expect(result.needsOnboarding()).toBe(!hasNode)
  pending.resolve(response({ success: true, data: nodes }))
  await rs.advanceTimersByTimeAsync(0)
  flush()
  expect(result.needsOnboarding()).toBe(!hasNode)
})

it('times out a stuck initial read and recovers through polling', async () => {
  rs.useFakeTimers()
  const fetchMock = rs.fn()
    .mockImplementationOnce(() => new Promise(() => {}))
    .mockResolvedValue(response({ success: true, data: statusMock({ running: true }) }))
  rs.stubGlobal('fetch', fetchMock)
  const { result } = renderHook(() => {
    const status = useStatus()
    usePolling([status.fetchStatus])
    return status
  })
  flush()
  await Promise.resolve()
  await rs.advanceTimersByTimeAsync(READ_TIMEOUT_MS)
  expect(result.statusSettled()).toBe(true)
  expect(result.statusLoaded()).toBe(false)
  await rs.advanceTimersByTimeAsync(3000)
  expect(result.statusLoaded()).toBe(true)
  expect(result.status().ready).toBe(true)
})

it('cancels the remaining batch queue when measurements are cleared', async () => {
  const fetchMock = rs.fn(() => new Promise<Response>(() => {}))
  rs.stubGlobal('fetch', fetchMock)
  const { result } = renderHook(() => useDelays())
  const batch = result.testGroupDelays('/api/clash', 'proxy', Array.from({ length: 20 }, (_, i) => `node-${i}`))
  expect(fetchMock).toHaveBeenCalledTimes(6)
  result.clearDelays()
  await batch
  expect(fetchMock).toHaveBeenCalledTimes(6)
  expect(result.testingGroup()).toBe('')
  expect(result.testingNodes()).toEqual({})
})

it('refreshes configuration on revision changes instead of every health poll', async () => {
  rs.useFakeTimers()
  let revision = 1
  const reads: string[] = []
  rs.stubGlobal('fetch', rs.fn(async (input: string) => {
    reads.push(input)
    if (input === '/api/status') return response({ success: true, data: statusMock({ data_revision: revision }) })
    return response({ success: true, data: input === '/api/version' ? {} : [] })
  }))
  renderHook(() => useAppData())
  flush()
  await rs.advanceTimersByTimeAsync(0)
  flush()
  expect(reads.filter((path) => path === '/api/subs')).toHaveLength(1)
  await rs.advanceTimersByTimeAsync(9000)
  expect(reads.filter((path) => path === '/api/subs')).toHaveLength(1)
  revision++
  await rs.advanceTimersByTimeAsync(3000)
  flush()
  expect(reads.filter((path) => path === '/api/subs')).toHaveLength(2)
})

it('evaluates startup, onboarding, and auto-probe gates without false positives', () => {
  expect(isInitialLoadComplete({
    statusSettled: true, statusLoaded: false, subsSettled: false, nodesSettled: false, rulesSettled: false,
  })).toBe(true)
  expect(resolveAppScreenMode({ firstLoadDone: true, statusLoaded: false, needsOnboarding: false }))
    .toBe('backend-unreachable')

  const baseOnboarding = {
    firstLoadDone: true,
    statusLoaded: true,
    backendUnreachable: false,
    initializing: false,
    ready: false,
    subsCurrent: true,
    nodesCurrent: true,
    rulesCurrent: true,
    subsAvailable: true,
    nodesAvailable: true,
    subsError: '',
    nodesError: '',
    rulesError: '',
    subsCount: 0,
    nodesCount: 0,
  }
  expect(shouldEnterOnboarding(baseOnboarding)).toBe(true)
  expect(shouldEnterOnboarding({ ...baseOnboarding, backendUnreachable: true })).toBe(false)
  expect(shouldEnterOnboarding({ ...baseOnboarding, nodesCurrent: false })).toBe(false)
  expect(shouldEnterOnboarding({ ...baseOnboarding, rulesError: 'timeout' })).toBe(false)
  expect(shouldEnterOnboarding({ ...baseOnboarding, subsCount: 1 })).toBe(false)

  expect(shouldAutoTestCurrentNode({
    ready: true, initializing: false, currentNodeName: 'hk-1', isUrlTestGroup: false, lastAutoTestedNode: '',
  })).toBe(true)
  expect(shouldAutoTestCurrentNode({
    ready: true, initializing: false, currentNodeName: 'hk-1', isUrlTestGroup: true, lastAutoTestedNode: '',
  })).toBe(false)
  expect(shouldAutoTestCurrentNode({
    ready: true, initializing: false, currentNodeName: 'hk-1', isUrlTestGroup: false, lastAutoTestedNode: 'hk-1',
  })).toBe(false)
})
