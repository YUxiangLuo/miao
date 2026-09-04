import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { isClashProxyGroup, useConnections } from './useClash'

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
