import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import App from './App'
import { nodeMock, ruleMock, statusMock, subMock } from './testFixtures'

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

describe('Harbor Console integration', () => {
  afterEach(() => {
    rs.useRealTimers()
    rs.unstubAllGlobals()
  })

  it('completes first-time SetupWizard onboarding and enters OverviewView', async () => {
    let hasSub = false
    const fetchMock = rs.fn(async (input, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: statusMock({ running: false, ready: false, phase: 'stopped' }),
        })
      }
      if (url === '/api/subs' && init?.method === 'POST') {
        hasSub = true
        return jsonResponse({ success: true, message: 'Subscription added' })
      }
      if (url === '/api/subs') {
        return jsonResponse({
          success: true,
          data: hasSub ? [subMock({ url: 'https://sub.example.com/yaml' })] : [],
        })
      }
      if (url === '/api/nodes' || url === '/api/rules' || url === '/api/terminals' || url === '/api/access-ports') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/version') {
        return jsonResponse({ success: true, data: { current: '0.50.0', latest: null, has_update: false } })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()
    const user = userEvent.setup()
    render(() => <App />)

    expect(await screen.findByRole('heading', { name: '欢迎使用 Harbor' })).toBeInTheDocument()
    await user.type(
      screen.getByPlaceholderText(/subscribe\?token=/),
      'https://sub.example.com/yaml',
    )
    await user.click(screen.getByRole('button', { name: '添加订阅并启动' }))

    expect(await screen.findByRole('heading', { name: '默认代理出口' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '概览' })).toHaveAttribute('aria-current', 'page')
  })

  it('degrades gracefully when connected to an older backend returning 404 on /api/terminals and /api/access-ports', async () => {
    rs.stubGlobal('fetch', rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes') return jsonResponse({ success: true, data: [nodeMock({ tag: 'vps-hk' })] })
      if (url === '/api/rules') {
        return jsonResponse({
          success: true,
          data: [ruleMock({ field: 'source_ip_cidr', value: '192.168.8.180/32', target: 'vps-hk' })],
        })
      }
      if (url === '/api/terminals' || url === '/api/access-ports') {
        return jsonResponse({ success: false, message: 'Not Found' }, 404)
      }
      if (url === '/api/version') {
        return jsonResponse({ success: true, data: { current: '0.49.4', latest: null, has_update: false } })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({ proxies: { proxy: { type: 'Selector', now: 'vps-hk', all: ['vps-hk'] } } })
      }
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url.includes('/delay')) return jsonResponse({ delay: 45 })
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    const user = userEvent.setup()
    render(() => <App />)

    expect(await screen.findByRole('heading', { name: '默认代理出口' })).toBeInTheDocument()

    const nav = screen.getByRole('navigation', { name: '主导航' })
    await user.click(within(nav).getByRole('button', { name: /设备与端口/ }))
    expect(await screen.findByText(/只读兼容模式/)).toBeInTheDocument()
    expect(screen.getByText('192.168.8.180')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /独立代理端口/ }))
    expect(await screen.findByText(/暂不支持独立代理端口/)).toBeInTheDocument()
  })

  it('surfaces blocked node alert banner and creates an independent proxy port via SelectPopover', async () => {
    let ports = [
      {
        id: 'ap-20101',
        name: '测试环境 01',
        port: 20101,
        outbound: 'uk-offline-node',
        has_auth: false,
        blocked: true,
      },
    ]

    const fetchMock = rs.fn(async (input, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes') {
        return jsonResponse({
          success: true,
          data: [
            nodeMock({ tag: 'us-socks-1', node_type: 'socks', detour: 'hk-01' }),
            nodeMock({ tag: 'jp-socks-2', node_type: 'socks' }),
          ],
        })
      }
      if (url === '/api/rules' || url === '/api/terminals') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/access-ports' && init?.method === 'POST') {
        const body = JSON.parse(String(init.body))
        const created = {
          id: `ap-${body.port}`,
          name: body.name,
          port: body.port,
          outbound: body.outbound,
          username: body.username,
          password: body.password,
          has_auth: Boolean(body.username && body.password),
          blocked: false,
        }
        ports = [...ports, created]
        return jsonResponse({ success: true, data: created })
      }
      if (url === '/api/access-ports') {
        return jsonResponse({ success: true, data: ports })
      }
      if (url === '/api/version') {
        return jsonResponse({ success: true, data: { current: '0.50.0', latest: null, has_update: false } })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: { proxy: { type: 'Selector', now: 'us-socks-1', all: ['us-socks-1', 'jp-socks-2'] } },
        })
      }
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url.includes('/delay')) return jsonResponse({ delay: 62 })
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()
    const user = userEvent.setup()
    render(() => <App />)

    expect(await screen.findByText(/有 1 项设备或独立端口所绑定的节点当前不可用/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '查看设备与端口' }))

    expect(await screen.findByText('测试环境 01')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '新建端口' }))
    await user.type(screen.getByPlaceholderText('例如：独立浏览器 01'), '独立浏览器 02')

    await user.click(screen.getByRole('button', { name: '绑定出口' }))
    await user.click(await screen.findByRole('option', { name: /jp-socks-2/ }))

    await user.click(screen.getByRole('button', { name: '创建端口' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/access-ports',
        expect.objectContaining({
          method: 'POST',
          body: expect.stringContaining('"outbound":"jp-socks-2"'),
        }),
      )
    })
    expect(await screen.findByText('独立浏览器 02')).toBeInTheDocument()
  })

  it('assigns LAN device policy, switches route mode from sidebar dock, and navigates via CommandPalette (⌘K)', async () => {
    let routeMode = 'rule'
    const fetchMock = rs.fn(async (input, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: statusMock({ running: true, ready: true, route_mode: routeMode as 'rule' | 'global' }),
        })
      }
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes') return jsonResponse({ success: true, data: [nodeMock({ tag: 'us-vip' })] })
      if (url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/terminals') {
        return jsonResponse({
          success: true,
          data: [{ ip: '192.168.8.214', name: 'NAS 主机', department: '书房', dhcp_hostname: 'NAS-01' }],
        })
      }
      if (url === '/api/terminals/assign' && init?.method === 'POST') {
        return jsonResponse({ success: true, message: 'Terminal routing updated' })
      }
      if (url === '/api/access-ports') return jsonResponse({ success: true, data: [] })
      if (url === '/api/route-mode' && init?.method === 'POST') {
        routeMode = JSON.parse(String(init.body)).route_mode
        return jsonResponse({ success: true, message: 'Updated' })
      }
      if (url === '/api/version') {
        return jsonResponse({ success: true, data: { current: '0.50.0', latest: null, has_update: false } })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({ proxies: { proxy: { type: 'Selector', now: 'us-vip', all: ['us-vip'] } } })
      }
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url.includes('/delay')) return jsonResponse({ delay: 38 })
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()
    const user = userEvent.setup()
    render(() => <App />)

    const nav = await screen.findByRole('navigation', { name: '主导航' })
    await user.click(within(nav).getByRole('button', { name: /设备与端口/ }))
    const seatPicker = await screen.findByRole('button', { name: /设置 NAS 主机 \(192\.168\.8\.214\) 出口策略/ })
    await user.click(seatPicker)
    await user.click(await screen.findByRole('option', { name: /禁止走代理/ }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/terminals/assign',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ ip: '192.168.8.214', mode: 'block_overseas' }),
        }),
      )
    })

    // Switch route mode from persistent sidebar control dock
    const sidebarModeGroup = screen.getByRole('group', { name: '分流模式切换' })
    fireEvent.click(within(sidebarModeGroup).getByRole('button', { name: '全局代理' }))
    flush()
    expect(await screen.findByRole('dialog', { name: '切换为全局代理' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '确认' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/route-mode',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ route_mode: 'global' }),
        }),
      )
    })

    // Open CommandPalette via Ctrl+K shortcut and jump to 节点与订阅
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true })
    expect(await screen.findByRole('dialog', { name: '命令面板' })).toBeInTheDocument()
    await user.type(screen.getByRole('textbox', { name: '搜索命令或节点' }), '节点与订阅')
    await user.click(await screen.findByRole('option', { name: /前往 节点与订阅/ }))

    expect(await screen.findByRole('region', { name: '订阅源' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: '手动节点' })).toBeInTheDocument()

    // Navigate to 系统设置 and verify MCP switch is accessible
    await user.click(screen.getByRole('button', { name: /系统设置/ }))
    expect(await screen.findByRole('switch', { name: 'MCP 端点开关' })).toBeInTheDocument()
  })
})
