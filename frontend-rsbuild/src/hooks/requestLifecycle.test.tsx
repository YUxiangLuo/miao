import { act, renderHook } from '@testing-library/react'
import { afterEach, expect, it, rs } from '@rstest/core'
import { useStatus, useNodes } from './useResources'
import { useDelays, useProxies } from './useClash'
import { useAppData } from './useAppData'
import { usePolling } from './usePolling'
import { READ_TIMEOUT_MS } from './request'
import { nodeMock, statusMock } from '../testFixtures'
function deferred() { let resolve!: (value: Response) => void; const promise = new Promise<Response>(done => {resolve=done}); return {resolve,promise} }
function response(data: unknown) { return {ok:true,json:async()=>data} as Response }
afterEach(()=>{rs.useRealTimers();rs.unstubAllGlobals()})
it('retains cached nodes on read failure and clears the error only after successful recovery', async () => {
  const node = nodeMock({ tag: 'cached-node' })
  const fetch = rs.fn()
    .mockResolvedValueOnce(response({ success: true, data: [node] }))
    .mockRejectedValueOnce(new Error('connection lost'))
    .mockResolvedValueOnce(response({ success: true, data: [] }))
  rs.stubGlobal('fetch', fetch)
  const { result } = renderHook(() => useNodes())
  await act(async () => { expect(await result.current.fetchNodes()).toEqual({ status: 'success', data: [node] }) })
  await act(async () => { expect(await result.current.fetchNodes()).toEqual({ status: 'error', error: 'connection lost' }) })
  expect(result.current.nodes).toEqual([node])
  expect(result.current.nodesError).toBe('connection lost')
  await act(async () => { await result.current.fetchNodes() })
  expect(result.current.nodesError).toBe('')
  expect(result.current.nodes).toEqual([])
})

it('does not publish an error from an obsolete list request', async () => {
  const old = deferred()
  rs.stubGlobal('fetch', rs.fn().mockImplementationOnce(() => old.promise)
    .mockResolvedValueOnce(response({ success: true, data: [] })))
  const { result } = renderHook(() => useNodes())
  let previous!: Promise<unknown>
  act(() => { previous = result.current.fetchNodes() })
  await act(async () => { await result.current.fetchNodes() })
  await act(async () => {
    old.resolve(response({ success: false, message: 'obsolete error' }))
    expect(await previous).toEqual({ status: 'superseded' })
  })
  expect(result.current.nodesError).toBe('')
  expect(result.current.nodesAvailable).toBe(true)
})

it('uses the request-start revision even through a retained refresh callback', async () => {
  const pending = deferred()
  const fetchMock = rs.fn().mockResolvedValue(response({ success: true, data: [] }))
  rs.stubGlobal('fetch', fetchMock)
  const { result, rerender } = renderHook(({ revision }) => useNodes(revision), { initialProps: { revision: 1 } })
  const retained = result.current.fetchNodes
  await act(async () => { await retained() })
  rerender({ revision: 2 })
  await act(async () => { await result.current.fetchNodes() })
  expect(result.current.nodesCurrent).toBe(true)
  await act(async () => { await retained() })
  expect(result.current.nodesCurrent).toBe(true)

  fetchMock.mockImplementationOnce(() => pending.promise)
  let inFlight!: Promise<unknown>
  act(() => { inFlight = retained() })
  rerender({ revision: 3 })
  await act(async () => { pending.resolve(response({ success: true, data: [] })); await inFlight })
  expect(result.current.nodesCurrent).toBe(false)
  await act(async () => { await retained() })
  expect(result.current.nodesCurrent).toBe(true)
})

it('keeps the latest status when an older response arrives last',async()=>{
 const old=deferred(),fresh=deferred(); rs.stubGlobal('fetch',rs.fn().mockImplementationOnce(()=>old.promise).mockImplementationOnce(()=>fresh.promise));
 const {result}=renderHook(()=>useStatus());let a!:Promise<unknown>,b!:Promise<unknown>;
 act(()=>{a=result.current.fetchStatus();b=result.current.fetchStatus()});
 await act(async()=>{fresh.resolve(response({success:true,data:statusMock({running:true})}));await b});
 expect(result.current.status.ready).toBe(true);
 await act(async()=>{old.resolve(response({success:true,data:statusMock({running:false})}));await a});
 expect(result.current.status.ready).toBe(true);
})
it('discards proxy responses from before the service stopped',async()=>{
 const old=deferred();rs.stubGlobal('fetch',rs.fn(()=>old.promise));
 const {result,rerender}=renderHook(({ready})=>useProxies({ready}),{initialProps:{ready:true}});let a!:Promise<unknown>;
 act(()=>{a=result.current.fetchProxies()});rerender({ready:false});
 expect(result.current.proxies).toEqual({});
 await act(async()=>{old.resolve(response({proxies:{proxy:{type:'Selector',all:['old-node'],now:'old-node'}}}));await a});
 expect(result.current.primaryGroup).toBeNull();
})
it('discards measurements after their generation is cleared',async()=>{
 const old=deferred();rs.stubGlobal('fetch',rs.fn(()=>old.promise));const {result}=renderHook(()=>useDelays());let a!:Promise<unknown>;
 act(()=>{a=result.current.testDelay('/api/clash','old-node')});act(()=>result.current.clearDelays());
 await act(async()=>{old.resolve(response({delay:123}));await a});
 expect(result.current.delays['old-node']).toBeUndefined();
})

it('gates fresh and retained proxy callbacks while stopped and allows restart', async () => {
  const fetchMock = rs.fn(async () => response({ proxies: { proxy: { type: 'Selector', all: ['node'], now: 'node' } } }))
  rs.stubGlobal('fetch', fetchMock)
  const { result, rerender } = renderHook(({ ready }) => useProxies({ ready }), { initialProps: { ready: true } })
  const retained = result.current.fetchProxies
  rerender({ ready: false })
  await act(async () => { await retained(); await result.current.fetchProxies() })
  expect(result.current.proxies).toEqual({})
  expect(fetchMock).not.toHaveBeenCalled()
  rerender({ ready: true })
  await act(async () => { await retained() })
  expect(result.current.primaryGroup?.now).toBe('node')
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
  await act(async () => { await rs.advanceTimersByTimeAsync(0) })
  expect(result.current.needsOnboarding).toBe(true)
  revision = 2
  await act(async () => { await result.current.fetchStatus() })
  expect(result.current.needsOnboarding).toBe(false)
  const nodes = hasNode ? [nodeMock({ tag: 'new-node' })] : []
  await act(async () => { pending.resolve(response({ success: true, data: nodes })); await rs.advanceTimersByTimeAsync(0) })
  expect(result.current.needsOnboarding).toBe(!hasNode)
  pending = deferred()
  await act(async () => { await rs.advanceTimersByTimeAsync(30_000) })
  expect(result.current.needsOnboarding).toBe(!hasNode)
  await act(async () => { pending.resolve(response({ success: true, data: nodes })); await rs.advanceTimersByTimeAsync(0) })
  expect(result.current.needsOnboarding).toBe(!hasNode)
})

it('times out a stuck initial read and recovers through polling', async () => {
  rs.useFakeTimers()
  const fetchMock = rs.fn().mockImplementationOnce(() => new Promise(() => {}))
    .mockResolvedValue(response({ success: true, data: statusMock({ running: true }) }))
  rs.stubGlobal('fetch', fetchMock)
  const { result } = renderHook(() => {
    const status = useStatus()
    usePolling([status.fetchStatus])
    return status
  })
  await act(async () => { await Promise.resolve() })
  await act(async () => { await rs.advanceTimersByTimeAsync(READ_TIMEOUT_MS) })
  expect(result.current.statusSettled).toBe(true)
  expect(result.current.statusLoaded).toBe(false)
  await act(async () => { await rs.advanceTimersByTimeAsync(3000) })
  expect(result.current.statusLoaded).toBe(true)
  expect(result.current.status.ready).toBe(true)
})

it('cancels the remaining batch queue when measurements are cleared', async () => {
  const fetchMock = rs.fn(() => new Promise<Response>(() => {}))
  rs.stubGlobal('fetch', fetchMock)
  const { result } = renderHook(() => useDelays())
  let batch!: Promise<void>
  act(() => { batch = result.current.testGroupDelays('/api/clash', 'proxy', Array.from({ length: 20 }, (_, i) => `node-${i}`)) })
  expect(fetchMock).toHaveBeenCalledTimes(6)
  await act(async () => { result.current.clearDelays(); await batch })
  expect(fetchMock).toHaveBeenCalledTimes(6)
  expect(result.current.testingGroup).toBe('')
  expect(result.current.testingNodes).toEqual({})
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
  await act(async () => { await rs.advanceTimersByTimeAsync(0) })
  expect(reads.filter(path => path === '/api/subs')).toHaveLength(1)
  await act(async () => { await rs.advanceTimersByTimeAsync(9000) })
  expect(reads.filter(path => path === '/api/subs')).toHaveLength(1)
  revision++
  await act(async () => { await rs.advanceTimersByTimeAsync(3000) })
  expect(reads.filter(path => path === '/api/subs')).toHaveLength(2)
})
