import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
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

describe('Harbor Studio Console integration', () => {
  afterEach(() => {
    rs.useRealTimers()
    rs.unstubAllGlobals()
  })

  it('completes first-time SetupWizard onboarding and enters StudioView', async () => {
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
          data: hasSub ? [subMock({ url: 'https://pool.example.com/sub' })] : [],
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

    expect(await screen.findByRole('heading', { name: '初始化 Harbor' })).toBeInTheDocument()
    await user.type(
      screen.getByPlaceholderText(/subscribe\?token=/),
      'https://pool.example.com/sub',
    )
    await user.click(screen.getByRole('button', { name: '接入订阅并启动网关' }))

    expect(await screen.findByText('局域网入口')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '工作台' })).toHaveAttribute('aria-current', 'page')
  })

  it('degrades gracefully when connected to an un-upgraded router returning 404 on /api/terminals and /api/access-ports', async () => {
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

    expect(await screen.findByText('局域网入口')).toBeInTheDocument()
    expect(await screen.findByText('192.168.8.180')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '终端与店铺' }))
    expect(await screen.findByText(/只读兼容模式/)).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /店铺端口/ }))
    expect(await screen.findByText(/尚未升级支持局域网多店铺专属端口/)).toBeInTheDocument()
  })

  it('surfaces fail-closed circuit-breaker banner and creates a dedicated store port via SelectPopover', async () => {
    let ports = [
      {
        id: 'ap-20101',
        name: 'TikTok 英区老店',
        port: 20101,
        outbound: 'uk-offline-isp',
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
            nodeMock({ tag: 'us-isp-1', node_type: 'socks', detour: 'hk-01' }),
            nodeMock({ tag: 'jp-isp-2', node_type: 'socks' }),
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
          proxies: { proxy: { type: 'Selector', now: 'us-isp-1', all: ['us-isp-1', 'jp-isp-2'] } },
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

    expect(await screen.findByText(/1 项独享绑定因目标专线离线已触发防关联熔断/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '查看终端与店铺' }))

    expect(await screen.findByText('TikTok 英区老店')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '新建店铺端口' }))
    await user.type(screen.getByPlaceholderText('例如：Amazon 美区一店'), 'Amazon 美区旗舰店')

    await user.click(screen.getByRole('button', { name: '绑定独享出口' }))
    await user.click(await screen.findByRole('option', { name: /jp-isp-2/ }))

    await user.click(screen.getByRole('button', { name: '创建并启用端口' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/access-ports',
        expect.objectContaining({
          method: 'POST',
          body: expect.stringContaining('"outbound":"jp-isp-2"'),
        }),
      )
    })
    expect(await screen.findByText('Amazon 美区旗舰店')).toBeInTheDocument()
  })

  it('assigns workstation seat policy, controls engine/route mode via GatewayPopover, and navigates via CommandPalette (⌘K)', async () => {
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
          data: [{ ip: '192.168.8.214', name: '王财务', department: '财务部', dhcp_hostname: 'CAIWU' }],
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

    await user.click(await screen.findByRole('button', { name: '终端与店铺' }))
    const seatPicker = await screen.findByRole('button', { name: /分配 王财务 \(192\.168\.8\.214\) 出口/ })
    await user.click(seatPicker)
    await user.click(await screen.findByRole('option', { name: /仅限国内办公/ }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/terminals/assign',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ ip: '192.168.8.214', mode: 'block_overseas' }),
        }),
      )
    })

    // Open GatewayPopover from topbar status pill
    await user.click(screen.getByRole('button', { name: /运行中/ }))
    expect(await screen.findByRole('dialog', { name: '网关引擎控制' })).toBeInTheDocument()
    expect(screen.getByRole('switch', { name: 'MCP 端点开关' })).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '全量出海排障' }))
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

    // Open CommandPalette via ⌘K / Ctrl+K shortcut and jump to 全球线路
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true })
    expect(await screen.findByRole('dialog', { name: '全局命令面板' })).toBeInTheDocument()
    await user.type(screen.getByRole('textbox', { name: '搜索命令或节点' }), '全球线路')
    await user.click(await screen.findByRole('option', { name: /前往 全球线路/ }))

    expect(await screen.findByRole('region', { name: '独享静态 IP' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: '公共出海池' })).toBeInTheDocument()
  })
})
