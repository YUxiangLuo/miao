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
    expect(await screen.findByText('订阅管理')).toBeInTheDocument()
  })
})
