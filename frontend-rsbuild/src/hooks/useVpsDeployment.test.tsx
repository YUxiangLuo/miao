import { act, renderHook } from '@testing-library/react'
import { afterEach, expect, it, rs } from '@rstest/core'
import { useApi } from './useApi'
import { useVpsDeployment } from './useVpsDeployment'
import { RequestInterruptedError } from './request'
import type { NodeInfo } from '../types/api'
import type { ResourceReadResult } from './useResources'

afterEach(() => { rs.unstubAllGlobals() })

function setup(fetchNodes = rs.fn<() => Promise<ResourceReadResult<NodeInfo[]>>>().mockResolvedValue({ status: 'success', data: [] })) {
  const clearDelays = rs.fn()
  const showToast = rs.fn().mockReturnValue(1)
  return { ...renderHook(() => useVpsDeployment({ ...useApi(), fetchNodes, clearDelays, showToast })), fetchNodes }
}

const request = { ip: '203.0.113.10', password: 'private-value' }
const ok = () => new Response(JSON.stringify({ success: true, message: '部署完成' }))

it('keeps the request locked through list synchronization and retains only non-secret task data', async () => {
  let resolveRequest!: (response: Response) => void
  let resolveNodes!: (nodes: ResourceReadResult<NodeInfo[]>) => void
  const fetch = rs.fn(() => new Promise<Response>(resolve => { resolveRequest = resolve }))
  rs.stubGlobal('fetch', fetch)
  const { result } = setup(rs.fn(() => new Promise<ResourceReadResult<NodeInfo[]>>(resolve => { resolveNodes = resolve })))
  let pending!: Promise<boolean>
  act(() => { pending = result.current.handleDeployVps(request) })
  expect(result.current.vpsTasks[0].phase).toBe('waiting')
  await act(async () => { expect(await result.current.handleDeployVps(request)).toBe(false) })
  await act(async () => { resolveRequest(ok()) })
  expect(result.current.vpsTasks[0].phase).toBe('syncing')
  expect(result.current.vpsDeploying).toBe(true)
  await act(async () => { expect(await result.current.handleDeployVps(request)).toBe(false) })
  await act(async () => { resolveNodes({ status: 'success', data: [] }); await pending })
  expect(result.current.vpsTasks[0].phase).toBe('success')
  expect(result.current.vpsDeploying).toBe(false)
  expect(JSON.stringify(result.current.vpsTasks)).not.toContain(request.password)
  expect(fetch).toHaveBeenCalledTimes(1)
})

it('does not misreport an unsynchronized list as a failed deployment and retains the latest ten records', async () => {
  rs.stubGlobal('fetch', rs.fn(async () => ok()))
  const { result } = setup(rs.fn().mockResolvedValue({ status: 'error', error: '读取失败' }))
  for (let i = 1; i <= 11; i++) {
    await act(async () => { await result.current.handleDeployVps({ ...request, ip: `203.0.113.${i}` }) })
  }
  expect(result.current.vpsTasks).toHaveLength(10)
  expect(result.current.vpsTasks[0]).toMatchObject({ host: '203.0.113.11', phase: 'success' })
  expect(result.current.vpsTasks[0].message).toContain('列表尚未同步')
  expect(result.current.vpsTasks[9].host).toBe('203.0.113.2')
})

it.each([
  [new RequestInterruptedError('timeout'), 'unknown'],
  [new TypeError('network failure'), 'unknown'],
  [new Error('SSH rejected'), 'error'],
])('classifies interrupted versus rejected requests: %s', async (error, phase) => {
  rs.stubGlobal('fetch', rs.fn().mockRejectedValue(error))
  const { result, fetchNodes } = setup()
  await act(async () => { await expect(result.current.handleDeployVps(request)).rejects.toThrow() })
  expect(result.current.vpsTasks[0].phase).toBe(phase)
  expect(result.current.vpsTasks[0].finishedAt).toBeDefined()
  expect(result.current.vpsDeploying).toBe(false)
  expect(fetchNodes).not.toHaveBeenCalled()
})
