import { act, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import App from './App'

function jsonResponse(payload: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => payload,
    text: async () => JSON.stringify(payload),
  }
}

function stubMatchMedia() {
  rs.stubGlobal('matchMedia', rs.fn().mockImplementation(() => ({
    matches: false,
    addEventListener: rs.fn(),
    removeEventListener: rs.fn(),
  })))
  window.matchMedia = globalThis.matchMedia
}

describe('App onboarding integration', () => {
  afterEach(() => {
    rs.useRealTimers()
    rs.unstubAllGlobals()
  })

  it('adds the first subscription and leaves onboarding', async () => {
    let hasSubscription = false
    const fetchMock = rs.fn(async (input, options = {}) => {
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

    rs.stubGlobal('fetch', fetchMock)
    rs.stubGlobal('matchMedia', rs.fn().mockImplementation(() => ({
      matches: false,
      addEventListener: rs.fn(),
      removeEventListener: rs.fn(),
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

  it('auto-tests the current node delay once when the dashboard loads', async () => {
    const delayCalls: string[] = []
    const fetchMock = rs.fn(async (input) => {
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
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
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
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    rs.stubGlobal('matchMedia', rs.fn().mockImplementation(() => ({
      matches: false,
      addEventListener: rs.fn(),
      removeEventListener: rs.fn(),
    })))
    window.matchMedia = globalThis.matchMedia

    render(<App />)

    // 面板渲染出来后,应自动对当前节点发起一次测速
    expect(await screen.findByText('节点列表')).toBeInTheDocument()
    await waitFor(() => {
      expect(delayCalls.some((url) => url.includes('node-a'))).toBe(true)
    })
    expect(delayCalls).toHaveLength(1)
    expect(delayCalls[0]).not.toContain('node-b')
  })

  it('does not auto-test delay in fastest mode and shows urltest history instead', async () => {
    const delayCalls: string[] = []
    const fetchMock = rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: { running: true, initializing: false, route_mode: 'rule', node_select: 'fastest_jp', pid: 1, uptime_secs: 10 },
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
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: {
            proxy: { type: 'URLTest', name: 'proxy', now: 'node-a', all: ['node-a', 'node-b'] },
            'node-a': { type: 'Hysteria2', name: 'node-a', history: [{ time: '2026-01-01T00:00:00Z', delay: 96 }] },
            'node-b': { type: 'Hysteria2', name: 'node-b', history: [{ time: '2026-01-01T00:00:00Z', delay: 120 }] },
          },
        })
      }
      if (url.startsWith('/api/clash/proxies/') && url.includes('/delay')) {
        delayCalls.push(url)
        return jsonResponse({ delay: 123 })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()

    render(<App />)

    // urltest 模式下不主动测速（测速会触发 sing-box 对 urltest 组即时重选，
    // 表现为面板打开几秒内连换节点），顶栏与节点瓷贴的延迟直接来自
    // /proxies 里 urltest 周期测速沉淀的 history
    expect(await screen.findByText('节点列表')).toBeInTheDocument()
    await waitFor(() => {
      expect(screen.getAllByText('96 ms').length).toBeGreaterThan(0)
      expect(screen.getAllByText('120 ms').length).toBeGreaterThan(0)
    })
    expect(delayCalls).toHaveLength(0)
  })

  it('can cancel a requested fastest strategy after the runtime falls back to manual', async () => {
    let requestedNodeSelect = 'fastest_jp'
    const fetchMock = rs.fn(async (input, options = {}) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: {
            running: true,
            ready: true,
            phase: 'ready',
            initializing: false,
            route_mode: 'rule',
            node_select: 'manual',
            requested_node_select: requestedNodeSelect,
            max_multiplier: '1',
            multiplier_options: ['1', '2.5'],
            warnings: [],
            mcp: false,
          },
        })
      }
      if (url === '/api/nodes' || url === '/api/subs' || url === '/api/rules') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: 'v0.44.5', latest: null, has_update: false },
        })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: {
            proxy: { type: 'Selector', name: 'proxy', now: '日本-01', all: ['日本-01'] },
            '日本-01': { type: 'Hysteria2', name: '日本-01' },
          },
        })
      }
      if (url.startsWith('/api/clash/proxies/') && url.includes('/delay')) {
        return jsonResponse({ delay: 100 })
      }
      if (url === '/api/node-select' && options.method === 'POST') {
        const body = JSON.parse(options.body)
        requestedNodeSelect = body.node_select
        return jsonResponse({ success: true, message: 'Node select updated' })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()

    const user = userEvent.setup()
    render(<App />)

    const select = await screen.findByRole('combobox', { name: '节点选择' })
    expect(select).toHaveValue('fastest_jp')
    await user.selectOptions(select, 'manual')

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/node-select', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ node_select: 'manual' }),
      }))
    })
  })

  it('asks for confirmation before switching route mode', async () => {
    let routeMode = 'rule'
    const fetchMock = rs.fn(async (input, options = {}) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: { running: true, initializing: false, route_mode: routeMode, pid: 1, uptime_secs: 10 },
        })
      }
      if (url === '/api/nodes' || url === '/api/subs' || url === '/api/rules') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: 'v0.29.5', latest: null, has_update: false },
        })
      }
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: {
            proxy: { type: 'Selector', name: 'proxy', now: 'node-a', all: ['node-a'] },
          },
        })
      }
      if (url.startsWith('/api/clash/proxies/') && url.includes('/delay')) {
        return jsonResponse({ delay: 80 })
      }
      if (url === '/api/route-mode' && options.method === 'POST') {
        const body = JSON.parse(options.body)
        routeMode = body.route_mode
        return jsonResponse({ success: true, message: 'Route mode updated' })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    rs.stubGlobal('matchMedia', rs.fn().mockImplementation(() => ({
      matches: false,
      addEventListener: rs.fn(),
      removeEventListener: rs.fn(),
    })))
    window.matchMedia = globalThis.matchMedia

    const user = userEvent.setup()
    render(<App />)

    expect(await screen.findByRole('button', { name: '全局代理' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '全局代理' }))

    expect(await screen.findByRole('dialog', { name: '切换为全局代理' })).toBeInTheDocument()
    expect(fetchMock).not.toHaveBeenCalledWith('/api/route-mode', expect.anything())

    await user.click(screen.getByRole('button', { name: '取消' }))
    expect(screen.queryByRole('dialog', { name: '切换为全局代理' })).not.toBeInTheDocument()
    expect(fetchMock).not.toHaveBeenCalledWith('/api/route-mode', expect.anything())

    await user.click(screen.getByRole('button', { name: '全局代理' }))
    await user.click(screen.getByRole('button', { name: '确认' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/route-mode', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ route_mode: 'global' }),
      }))
    })
    expect(await screen.findByText('已切换为全局代理')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '分流模式' }))
    expect(await screen.findByRole('dialog', { name: '切换为分流模式' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '确认' }))
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/route-mode', expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ route_mode: 'rule' }),
      }))
    })
    expect(await screen.findByText('已切换为分流模式')).toBeInTheDocument()
  })

  it('shows a reconnecting state instead of onboarding when the backend is unreachable', async () => {
    rs.useFakeTimers()
    let backendDown = true
    const fetchMock = rs.fn(async (input) => {
      const url = String(input)
      if (backendDown) throw new Error('connection refused')
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: { running: false, initializing: false, route_mode: 'rule' },
        })
      }
      if (url === '/api/nodes' || url === '/api/subs' || url === '/api/rules') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: 'v0.29.5', latest: null, has_update: false },
        })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()

    render(<App />)
    await act(async () => {})

    // 后端不可达：显示重连提示，而不是把默认空状态误判成引导页
    expect(screen.getByText('无法连接后端，正在自动重试…')).toBeInTheDocument()
    expect(screen.queryByText('添加订阅链接或手动节点以开始使用')).not.toBeInTheDocument()

    // 后台轮询仍在继续重试
    const statusCallsBefore = fetchMock.mock.calls.filter(([url]) => url === '/api/status').length
    await act(async () => { rs.advanceTimersByTime(6000) })
    const statusCallsAfter = fetchMock.mock.calls.filter(([url]) => url === '/api/status').length
    expect(statusCallsAfter).toBeGreaterThan(statusCallsBefore)
    expect(screen.getByText('无法连接后端，正在自动重试…')).toBeInTheDocument()

    // 后端恢复后自动进入正常流程（空数据 → 引导页）
    backendDown = false
    await act(async () => { rs.advanceTimersByTime(3000) })
    expect(screen.getByText('添加订阅链接或手动节点以开始使用')).toBeInTheDocument()
    expect(screen.queryByText('无法连接后端，正在自动重试…')).not.toBeInTheDocument()
  })

  it('shows a disconnect banner after repeated polling failures and hides it after recovery', async () => {
    rs.useFakeTimers()
    let backendDown = false
    const fetchMock = rs.fn(async (input) => {
      const url = String(input)
      if (backendDown) throw new Error('connection refused')
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
          data: { current: 'v0.29.5', latest: null, has_update: false },
        })
      }
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: {
            proxy: { type: 'Selector', name: 'proxy', now: 'node-a', all: ['node-a'] },
          },
        })
      }
      if (url.startsWith('/api/clash/proxies/') && url.includes('/delay')) {
        return jsonResponse({ delay: 80 })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()

    render(<App />)
    await act(async () => {})
    expect(screen.getByText('节点列表')).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()

    // 偶发失败不提示（阈值 3 次）
    backendDown = true
    await act(async () => { rs.advanceTimersByTime(3000) })
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    await act(async () => { rs.advanceTimersByTime(3000) })
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()

    // 连续失败达到阈值：出现断线横幅，且停留在仪表盘而不是跳转引导页
    await act(async () => { rs.advanceTimersByTime(3000) })
    expect(screen.getByRole('alert')).toHaveTextContent('与后端服务的连接已断开，正在自动重试…')
    expect(screen.getByText('节点列表')).toBeInTheDocument()
    expect(screen.queryByText('添加订阅链接或手动节点以开始使用')).not.toBeInTheDocument()

    // 恢复后轮询成功，横幅自动消失
    backendDown = false
    await act(async () => { rs.advanceTimersByTime(3000) })
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(screen.getByText('节点列表')).toBeInTheDocument()
  })

  it('lets the user retry a failed proxy service', async () => {
    let started = false
    const fetchMock = rs.fn(async (input, options = {}) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: {
            running: started,
            ready: started,
            phase: started ? 'ready' : 'failed',
            initializing: false,
            route_mode: 'rule',
          },
        })
      }
      if (url === '/api/nodes') {
        return jsonResponse({
          success: true,
          data: [{ tag: 'node-a', server: 'example.com', server_port: 443, node_type: 'hysteria2' }],
        })
      }
      if (url === '/api/subs' || url === '/api/rules') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/version') {
        return jsonResponse({
          success: true,
          data: { current: 'v0.44.6', latest: null, has_update: false },
        })
      }
      if (url === '/api/service/start' && options.method === 'POST') {
        started = true
        return jsonResponse({ success: true, message: 'sing-box started successfully' })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: {
            proxy: { type: 'Selector', name: 'proxy', now: 'node-a', all: ['node-a'] },
          },
        })
      }
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url.startsWith('/api/clash/proxies/') && url.includes('/delay')) {
        return jsonResponse({ delay: 80 })
      }
      throw new Error(`Unexpected request: ${url}`)
    })
    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()
    const user = userEvent.setup()
    render(<App />)

    const retry = await screen.findByRole('button', { name: '重新启动' })
    await user.click(retry)

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/service/start', expect.objectContaining({ method: 'POST' }))
    })
    expect(await screen.findByText('代理服务已重新启动')).toBeInTheDocument()
    await waitFor(() => expect(screen.queryByRole('button', { name: '重新启动' })).not.toBeInTheDocument())
  })
})
