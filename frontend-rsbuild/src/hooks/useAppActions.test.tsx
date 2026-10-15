import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, expect, it, rs } from '@rstest/core'
import { useAppData } from './useAppData'
import { useAppActions } from './useAppActions'
import { statusMock } from '../testFixtures'
import type { NodeSelectResult } from '../types/api'

afterEach(() => { rs.unstubAllGlobals() })

function response(data: unknown) {
  return new Response(JSON.stringify(data))
}

function setup(onFetch: (url: string, options: RequestInit) => Response | Promise<Response> | undefined) {
  const fetch = rs.fn((input: string, options: RequestInit = {}) => {
    const handled = onFetch(input, options)
    if (handled) return Promise.resolve(handled)
    if (input === '/api/status') return Promise.resolve(response({ success: true, data: statusMock() }))
    if (input === '/api/version') return Promise.resolve(response({ success: true, data: { current: '0.49.2', latest: null, has_update: false } }))
    if (input === '/api/clash/proxies') return Promise.resolve(response({ proxies: {} }))
    return Promise.resolve(response({ success: true, data: [] }))
  })
  rs.stubGlobal('fetch', fetch)
  return { fetch, ...renderHook(() => {
    const data = useAppData()
    return { ...data, ...useAppActions(data) }
  }) }
}

it.each(['nodes', 'subs', 'rules'])('keeps a successful %s addition successful when list readback fails', async (resource) => {
  const endpoint = `/api/${resource}`
  let written = false
  let failRead = true
  const { result, fetch } = setup((url, options) => {
    if (url !== endpoint) return
    if (options.method === 'POST') {
      written = true
      return response({ success: true })
    }
    if (written && failRead) return response({ success: false, message: '读取失败' })
  })
  await waitFor(() => expect(result.current.firstLoadDone).toBe(true))
  act(() => { result.current.setNodeForm(form => ({ ...form, tag: 'fixture', server: 'example.com', password: 'fixture-password', server_port: 443 })) })
  await act(async () => {
    const success = resource === 'nodes' ? await result.current.handleAddNode()
      : resource === 'subs' ? await result.current.handleAddSubscription('https://example.com/sub')
        : await result.current.handleAddRule({ field: 'domain', value: 'example.com', target: 'direct' })
    expect(success).toBe(true)
  })
  expect(result.current.toasts.at(-1)).toMatchObject({ tone: 'info', message: expect.stringContaining('列表尚未同步') })
  const error = resource === 'nodes' ? result.current.nodesError : resource === 'subs' ? result.current.subsError : result.current.rulesError
  expect(error).toBe('读取失败')
  failRead = false
  await act(async () => {
    await (resource === 'nodes' ? result.current.fetchNodes() : resource === 'subs' ? result.current.fetchSubs() : result.current.fetchRules())
  })
  expect(fetch.mock.calls.filter(([, options]) => options?.method === 'POST')).toHaveLength(1)
})

it('does not label a superseded read as a failed list synchronization or repeat the write', async () => {
  let readsAfterWrite = 0
  let written = false
  const { result, fetch } = setup((url, options) => {
    if (url !== '/api/subs') return
    if (options.method === 'POST') { written = true; return response({ success: true }) }
    if (written && ++readsAfterWrite === 1) return new Promise<Response>(() => {})
  })
  await waitFor(() => expect(result.current.firstLoadDone).toBe(true))
  let adding!: Promise<boolean>
  act(() => { adding = result.current.handleAddSubscription('https://example.com/sub') })
  await waitFor(() => expect(readsAfterWrite).toBe(1))
  await act(async () => { await result.current.fetchSubs(); expect(await adding).toBe(true) })
  expect(result.current.subsError).toBe('')
  expect(result.current.toasts.at(-1)?.message).toContain('请查看最新列表')
  expect(result.current.toasts.at(-1)?.message).not.toContain('尚未同步')
  expect(fetch.mock.calls.filter(([, options]) => options?.method === 'POST')).toHaveLength(1)
})

it.each([
  ['fallback with changed wording', 'new backend wording', 'manual', '请求的地区暂无可用节点，当前使用手动选择'],
  ['success with legacy warning text', '该地区没有可用节点，已切回手动选择', 'fastest_jp', '节点选择已更新'],
] as const)('uses strategy fields instead of the message: %s', async (_, message, effective, expected) => {
  const data: NodeSelectResult = { requested: 'fastest_jp', node_select: effective, changed: true, runtime_updated: true, started: false, reloaded: true, restarted: false }
  const { result, fetch } = setup((url, options) => {
    if (url === '/api/node-select' && options.method === 'POST') return response({ success: true, message, data })
  })
  await waitFor(() => expect(result.current.firstLoadDone).toBe(true))
  await act(async () => { await result.current.handleSetNodeSelect('fastest_jp') })
  expect(result.current.toasts.at(-1)?.message).toBe(expected)
  const writes = fetch.mock.calls.filter(([, options]) => options?.method === 'POST')
  expect(writes).toHaveLength(1)
  expect(writes[0][1]?.body).toBe(JSON.stringify({ node_select: 'fastest_jp' }))
})

it('reports an incomplete strategy response without assuming the strategy took effect', async () => {
  const { result } = setup((url) => url === '/api/node-select' ? response({ success: true, message: 'accepted' }) : undefined)
  await waitFor(() => expect(result.current.firstLoadDone).toBe(true))
  await act(async () => { await result.current.handleSetNodeSelect('fastest_jp') })
  expect(result.current.toasts.at(-1)).toMatchObject({ tone: 'info', message: '节点选择请求已处理，请查看当前策略' })
})

it('leaves a status warning as the sole fallback notification', async () => {
  let requested = false
  const warning = '地区节点不可用，已采用手动策略'
  const { result } = setup((url) => {
    if (url === '/api/node-select') {
      requested = true
      return response({ success: true, data: { requested: 'fastest_jp', node_select: 'manual' } })
    }
    if (url === '/api/status' && requested) return response({ success: true, data: statusMock({ warning }) })
  })
  await waitFor(() => expect(result.current.firstLoadDone).toBe(true))
  await act(async () => { await result.current.handleSetNodeSelect('fastest_jp') })
  expect(result.current.toasts).toHaveLength(1)
  expect(result.current.toasts[0].message).toBe(warning)
})

it('preserves successful batch indices even if the refreshed list is unavailable', async () => {
  let written = false
  const added = [{ index: 0, tag: 'fixture' }]
  const { result } = setup((url) => {
    if (url === '/api/nodes/import') { written = true; return response({ success: true, data: { added, failed: [] } }) }
    if (url === '/api/nodes' && written) return response({ success: false, message: '读取失败' })
  })
  await waitFor(() => expect(result.current.firstLoadDone).toBe(true))
  await act(async () => {
    expect(await result.current.handleImportNodes([{ tag: 'fixture', node_type: 'hysteria2', server: 'example.com', server_port: 443, password: 'fixture-password' }])).toEqual({ added, failed: [] })
  })
  expect(result.current.toasts.at(-1)?.message).toContain('勿重复导入')
})
