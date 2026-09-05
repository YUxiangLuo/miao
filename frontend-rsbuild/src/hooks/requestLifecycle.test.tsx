import { act, renderHook } from '@testing-library/react'
import { afterEach, expect, it, rs } from '@rstest/core'
import { useStatus } from './useResources'
import { useDelays, useProxies } from './useClash'
import { useAppData } from './useAppData'
import { usePolling } from './usePolling'
import { READ_TIMEOUT_MS } from './request'
import { statusMock } from '../testFixtures'
function deferred() { let resolve!: (value: Response) => void; const promise = new Promise<Response>(done => {resolve=done}); return {resolve,promise} }
function response(data: unknown) { return {ok:true,json:async()=>data} as Response }
afterEach(()=>{rs.useRealTimers();rs.unstubAllGlobals()})
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
