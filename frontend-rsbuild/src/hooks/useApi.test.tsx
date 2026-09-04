import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { useApi } from './useApi'

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
    rs.unstubAllGlobals()
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
