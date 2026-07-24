import { StrictMode } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.jsx'

function jsonResponse(payload) {
  return {
    ok: true,
    status: 200,
    json: async () => payload,
    text: async () => JSON.stringify(payload),
  }
}

describe('App startup', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('does not duplicate initial data requests in StrictMode', async () => {
    const fetchMock = vi.fn(async (url) => {
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: {
            running: false,
            pid: null,
            uptime_secs: null,
            initializing: false,
            route_mode: 'rule',
          },
        })
      }
      if (url === '/api/subs') return jsonResponse({ success: true, data: [] })
      if (url === '/api/nodes') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: '1.0.0', latest: null, has_update: false },
        })
      }
      throw new Error(`Unexpected request: ${url}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    render(
      <StrictMode>
        <App />
      </StrictMode>
    )

    await screen.findByRole('heading', { name: 'Miao' })
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/version')
    })

    const requestedUrls = fetchMock.mock.calls.map(([url]) => url)
    expect(requestedUrls.filter((url) => url === '/api/status')).toHaveLength(1)
    expect(requestedUrls.filter((url) => url === '/api/subs')).toHaveLength(1)
    expect(requestedUrls.filter((url) => url === '/api/nodes')).toHaveLength(1)
    expect(requestedUrls.filter((url) => url === '/api/version')).toHaveLength(1)
  })

  it('waits for the initial status before loading configuration data', async () => {
    let resolveStatus
    const statusResponse = new Promise((resolve) => {
      resolveStatus = resolve
    })
    const fetchMock = vi.fn(async (url) => {
      if (url === '/api/status') return statusResponse
      if (url === '/api/subs') return jsonResponse({ success: true, data: [] })
      if (url === '/api/nodes') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: '1.0.0', latest: null, has_update: false },
        })
      }
      throw new Error(`Unexpected request: ${url}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    render(<App />)

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/status')
    })
    expect(fetchMock.mock.calls.some(([url]) => url === '/api/subs')).toBe(false)
    expect(fetchMock.mock.calls.some(([url]) => url === '/api/nodes')).toBe(false)

    resolveStatus(jsonResponse({
      success: true,
      data: {
        running: false,
        pid: null,
        uptime_secs: null,
        initializing: false,
        route_mode: 'rule',
      },
    }))

    await screen.findByRole('heading', { name: 'Miao' })
    expect(fetchMock).toHaveBeenCalledWith('/api/subs')
    expect(fetchMock).toHaveBeenCalledWith('/api/nodes')
  })
})
