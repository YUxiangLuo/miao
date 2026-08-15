import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import App from './App.jsx'

function jsonResponse(payload, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => payload,
    text: async () => JSON.stringify(payload),
  }
}

describe('App onboarding integration', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('adds the first subscription and leaves onboarding', async () => {
    let hasSubscription = false
    const fetchMock = vi.fn(async (input, options = {}) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: { running: false, initializing: false, route_mode: 'rule' },
        })
      }
      if (url === '/api/nodes') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/subs' && options.method === 'POST') {
        hasSubscription = true
        return jsonResponse({ success: true, message: 'Subscription added' })
      }
      if (url === '/api/subs') {
        return jsonResponse({
          success: true,
          data: hasSubscription
            ? [{ url: 'https://example.com/sub', success: true, node_count: 1 }]
            : [],
        })
      }
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: 'v0.27.0', latest: null, has_update: false },
        })
      }
      if (url === '/api/map/snapshot') {
        return jsonResponse({
          success: true,
          data: { client: { type: 'client', name: 'This Device' }, proxies: [], flows: [] },
        })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    vi.stubGlobal('fetch', fetchMock)
    vi.stubGlobal('matchMedia', vi.fn().mockImplementation(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })))
    window.matchMedia = globalThis.matchMedia

    const user = userEvent.setup()
    render(<App />)

    expect(await screen.findByText('添加订阅链接或手动节点以开始使用')).toBeInTheDocument()
    await user.type(screen.getByPlaceholderText('粘贴订阅链接...'), 'https://example.com/sub')
    await user.click(screen.getByRole('button', { name: '添加订阅' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/subs', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ url: 'https://example.com/sub' }),
      }))
    })
    expect(await screen.findByText('订阅已添加')).toBeInTheDocument()
    expect(await screen.findByLabelText('世界网络地图')).toBeInTheDocument()
  })

  it('auto-tests the current node delay once when the dashboard loads', async () => {
    const delayCalls = []
    const fetchMock = vi.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: { running: true, initializing: false, route_mode: 'rule', pid: 1, uptime_secs: 10 },
        })
      }
      if (url === '/api/nodes' || url === '/api/subs' || url === '/api/rules') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: 'v0.29.0', latest: null, has_update: false },
        })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: {
            proxy: { type: 'Selector', name: 'proxy', now: 'node-a', all: ['node-a', 'node-b'] },
          },
        })
      }
      if (url.startsWith('/api/clash/proxies/') && url.includes('/delay')) {
        delayCalls.push(url)
        return jsonResponse({ delay: 123 })
      }
      if (url === '/api/map/snapshot') {
        return jsonResponse({
          success: true,
          data: { client: { type: 'client', name: 'This Device' }, proxies: [], flows: [] },
        })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    vi.stubGlobal('fetch', fetchMock)
    vi.stubGlobal('matchMedia', vi.fn().mockImplementation(() => ({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })))
    window.matchMedia = globalThis.matchMedia

    render(<App />)

    // 地图作为默认主视图渲染后,仍自动对当前节点发起一次测速
    expect(await screen.findByLabelText('世界网络地图')).toBeInTheDocument()
    await waitFor(() => {
      expect(delayCalls.some((url) => url.includes('node-a'))).toBe(true)
    })
    expect(delayCalls).toHaveLength(1)
    expect(delayCalls[0]).not.toContain('node-b')
  })
})
