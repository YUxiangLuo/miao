import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { useApi, useToast } from './useApi'
import { WRITE_TIMEOUT_MS } from './request'

function deferredResponse() {
  let resolve!: (response: Response) => void
  const promise = new Promise<Response>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function successResponse(): Response {
  return {
    ok: true,
    json: async () => ({ success: true, message: 'ok' }),
  } as Response
}

describe('useApi pending actions', () => {
  afterEach(() => {
    rs.useRealTimers()
    rs.unstubAllGlobals()
  })

  it('keeps errors until dismissed, deduplicates them and expires ordinary notices', async () => {
    rs.useFakeTimers()
    const { result, unmount } = renderHook(() => useToast())
    let errorId = 0
    act(() => {
      result.current.showToast('已保存', 'success')
      errorId = result.current.showToast('订阅更新失败，旧节点仍可用', 'error')
      expect(result.current.showToast('订阅更新失败，旧节点仍可用', 'error')).toBe(errorId)
    })
    expect(result.current.toasts).toHaveLength(2)
    await act(async () => { await rs.advanceTimersByTimeAsync(3500) })
    expect(result.current.toasts.map(toast => toast.id)).toEqual([errorId])
    await act(async () => { await rs.advanceTimersByTimeAsync(60_000) })
    expect(result.current.toasts).toHaveLength(1)
    act(() => { result.current.dismissToast(errorId) })
    expect(result.current.toasts).toHaveLength(0)
    act(() => { result.current.showToast('检查中') })
    unmount()
    expect(rs.getTimerCount()).toBe(0)
  })

  it('bounds a hanging response body, releases pending and reconciles without retrying the write', async () => {
    rs.useFakeTimers()
    const reconcile = rs.fn()
    const fetchMock = rs.fn(async () => ({ ok: true, json: () => new Promise(() => {}) }))
    rs.stubGlobal('fetch', fetchMock)
    const { result } = renderHook(() => useApi(reconcile))
    let outcome!: Promise<unknown>
    act(() => {
      outcome = result.current.apiCall('nodes', { method: 'POST' }, 'saveNode').catch(error => error)
    })
    await act(async () => { await rs.advanceTimersByTimeAsync(WRITE_TIMEOUT_MS - 1) })
    expect(result.current.pendingActions.has('saveNode')).toBe(true)
    await act(async () => { await rs.advanceTimersByTimeAsync(1) })
    expect((await outcome as Error).message).toContain('操作结果未知')
    expect(result.current.pendingActions.size).toBe(0)
    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('reconciles a lost POST response without retrying the write', async () => {
    const reconcile = rs.fn()
    const fetchMock = rs.fn().mockRejectedValue(new TypeError('Failed to fetch'))
    rs.stubGlobal('fetch', fetchMock)
    const { result } = renderHook(() => useApi(reconcile))
    await act(async () => {
      await expect(result.current.apiCall('nodes', { method: 'POST' }, 'saveNode')).rejects.toThrow('操作结果未知')
    })
    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(fetchMock).toHaveBeenCalledTimes(1)
    expect(result.current.pendingActions.size).toBe(0)
  })

  it('preserves a server rejection message and clears pending', async () => {
    rs.stubGlobal('fetch', rs.fn(async () => new Response(JSON.stringify({ success: false, message: '节点不可用' }), { status: 400 })))
    const reconcile = rs.fn()
    const { result } = renderHook(() => useApi(reconcile))
    await act(async () => {
      await expect(result.current.apiCall('nodes', { method: 'POST' }, 'saveNode')).rejects.toThrow('节点不可用')
    })
    expect(result.current.pendingActions.size).toBe(0)
    expect(reconcile).not.toHaveBeenCalled()
  })

  it('tracks different actions independently', async () => {
    const first = deferredResponse()
    const second = deferredResponse()
    rs.stubGlobal('fetch', rs.fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise))
    const { result } = renderHook(() => useApi())

    let firstCall!: Promise<unknown>
    let secondCall!: Promise<unknown>
    act(() => {
      firstCall = result.current.apiCall('first', {}, 'first')
      secondCall = result.current.apiCall('second', {}, 'second')
    })
    expect(result.current.pendingActions).toEqual(new Set(['first', 'second']))

    await act(async () => {
      first.resolve(successResponse())
      await firstCall
    })
    expect(result.current.pendingActions).toEqual(new Set(['second']))

    await act(async () => {
      second.resolve(successResponse())
      await secondCall
    })
    expect(result.current.pendingActions.size).toBe(0)
  })

  it('keeps an action pending until every matching call finishes', async () => {
    const first = deferredResponse()
    const second = deferredResponse()
    rs.stubGlobal('fetch', rs.fn()
      .mockImplementationOnce(() => first.promise)
      .mockImplementationOnce(() => second.promise))
    const { result } = renderHook(() => useApi())

    let firstCall!: Promise<unknown>
    let secondCall!: Promise<unknown>
    act(() => {
      firstCall = result.current.apiCall('subs', {}, 'refreshSubs')
      secondCall = result.current.apiCall('subs', {}, 'refreshSubs')
    })

    await act(async () => {
      first.resolve(successResponse())
      await firstCall
    })
    expect(result.current.pendingActions.has('refreshSubs')).toBe(true)

    await act(async () => {
      second.resolve(successResponse())
      await secondCall
    })
    expect(result.current.pendingActions.has('refreshSubs')).toBe(false)
  })
})
