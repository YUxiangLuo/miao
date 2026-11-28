import { fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import App from './App'
import { resetMotionPolicyForTest } from './motion'
import { nodeMock, ruleMock, statusMock, subMock } from './testFixtures'

function jsonResponse(payload: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => payload,
    text: async () => JSON.stringify(payload),
  }
}

function stubMatchMedia(mobile = false) {
  rs.stubGlobal('matchMedia', rs.fn().mockImplementation(() => ({
    matches: mobile,
    addEventListener: rs.fn(),
    removeEventListener: rs.fn(),
  })))
  window.matchMedia = globalThis.matchMedia
}

describe('Harbor Console integration', () => {
  afterEach(() => {
    rs.useRealTimers()
    rs.unstubAllGlobals()
    resetMotionPolicyForTest()
  })

  it('completes first-time SetupWizard onboarding and enters streamlined NodesView', async () => {
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
        return jsonResponse({ success: true, data: { current: '0.51.3', latest: null, has_update: false } })
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

    expect(await screen.findByRole('heading', { name: '代理节点' })).toBeInTheDocument()
    const nav = screen.getByRole('navigation', { name: '主导航' })
    expect(within(nav).getAllByRole('button')).toHaveLength(4)
    expect(within(nav).getByRole('button', { name: /代理节点/ })).toHaveAttribute('aria-current', 'page')
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
    await user.click(within(nav).getByRole('button', { name: /分流隔离/ }))
    expect(await screen.findByText(/只读兼容模式/)).toBeInTheDocument()
    expect(screen.getByText('192.168.8.180')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /独立代理端口/ }))
    expect(await screen.findByText(/暂不支持独立代理端口/)).toBeInTheDocument()
  })

  it('surfaces global circuit-breaker banner, deep-links into blocked port tab, and creates port + rule', async () => {
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
    let rules = [] as ReturnType<typeof ruleMock>[]

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
      if (url === '/api/terminals') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/rules' && init?.method === 'POST') {
        const body = JSON.parse(String(init.body))
        const next = ruleMock({ index: rules.length, field: body.field, value: body.value, target: body.target })
        rules = [...rules, next]
        return jsonResponse({ success: true, message: 'Rule added' })
      }
      if (url === '/api/rules') {
        return jsonResponse({ success: true, data: rules })
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
        return jsonResponse({ success: true, data: { current: '0.51.3', latest: null, has_update: false } })
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

    expect(await screen.findByText(/有 1 项设备、独立端口或规则所绑定的节点当前不可用/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '查看分流隔离' }))

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

    // Switch to custom rules sub-tab within 分流隔离 and add a rule
    await user.click(screen.getByRole('button', { name: /自定义规则/ }))
    await user.click(screen.getByRole('button', { name: '新建规则' }))
    await user.type(screen.getByLabelText('匹配值'), 'openai.com')
    await user.click(screen.getByRole('button', { name: '添加规则' }))

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/rules',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ field: 'domain_suffix', value: 'openai.com', target: 'proxy' }),
        }),
      )
    })
    expect(await screen.findByText('openai.com')).toBeInTheDocument()
  })

  it('assigns LAN device policy, switches route mode from sidebar dock, jumps via CommandPalette (⌘K), and keeps Settings free of duplicate controls', async () => {
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
        return jsonResponse({ success: true, data: { current: '0.51.3', latest: null, has_update: false } })
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
    await user.click(within(nav).getByRole('button', { name: /分流隔离/ }))
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

    // Switch route mode from persistent sidebar control dock (single global home)
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

    // Open CommandPalette via Ctrl+K shortcut and jump directly to 节点源管理
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true })
    expect(await screen.findByRole('dialog', { name: '命令面板' })).toBeInTheDocument()
    await user.type(screen.getByRole('textbox', { name: '搜索命令或节点' }), '节点源管理')
    await user.click(await screen.findByRole('option', { name: /前往 节点源管理/ }))

    expect(await screen.findByRole('region', { name: '订阅源' })).toBeInTheDocument()
    expect(screen.getByRole('region', { name: '手动节点' })).toBeInTheDocument()

    // Navigate to 系统设置 and verify MCP switch is accessible & no duplicate route-mode group exists
    await user.click(within(nav).getByRole('button', { name: /系统设置/ }))
    expect(await screen.findByRole('switch', { name: 'MCP 端点开关' })).toBeInTheDocument()
    expect(screen.queryByRole('group', { name: '分流模式设置' })).toBeNull()
    expect(screen.getAllByRole('group', { name: '分流模式切换' })).toHaveLength(1)

    // Verify 4-state motion policy selector in Settings persists and syncs root dataset
    const motionGroup = screen.getByRole('group', { name: '界面动效' })
    await user.click(within(motionGroup).getByRole('button', { name: '精简' }))
    expect(document.documentElement.dataset.motionProfile).toBe('reduced')
    expect(window.localStorage.getItem('harbor-motion')).toBe('reduced')
  })

  it('supports independent single-node probe, cancellable batch speed test, latency sort, and 1-click auto-to-manual exit lock', async () => {
    const jpSlow = '🇯🇵 东京标准 01 [1.5x]'
    const jpFast = '🇯🇵 大阪极速 02'
    const jpStuck = '🇯🇵 名古屋专线 03'
    let nodeSelect: 'fastest_jp' | 'manual' = 'fastest_jp'
    let currentExit = jpSlow
    let resolveStuckProbe!: (value: ReturnType<typeof jsonResponse>) => void
    const stuckProbe = new Promise<ReturnType<typeof jsonResponse>>((resolve) => {
      resolveStuckProbe = resolve
    })

    const fetchMock = rs.fn(async (input, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: statusMock({
            running: true,
            ready: true,
            node_select: nodeSelect,
            requested_node_select: nodeSelect,
          }),
        })
      }
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes' || url === '/api/rules' || url === '/api/terminals' || url === '/api/access-ports') {
        return jsonResponse({ success: true, data: [] })
      }
      if (url === '/api/version') {
        return jsonResponse({ success: true, data: { current: '0.51.3', latest: null, has_update: false } })
      }
      if (url === '/api/clash/proxies') {
        return jsonResponse({
          proxies: {
            proxy: {
              type: nodeSelect === 'manual' ? 'Selector' : 'URLTest',
              now: currentExit,
              all: [jpSlow, jpFast, jpStuck],
            },
            [jpSlow]: { type: 'Hysteria2' },
            [jpFast]: { type: 'VLESS' },
            [jpStuck]: { type: 'Trojan' },
          },
        })
      }
      if (url === '/api/clash/connections') {
        return jsonResponse({ connections: [], uploadTotal: 0, downloadTotal: 0 })
      }
      if (url.includes(encodeURIComponent(jpSlow)) && url.includes('/delay')) {
        return jsonResponse({ delay: 165 })
      }
      if (url.includes(encodeURIComponent(jpFast)) && url.includes('/delay')) {
        return jsonResponse({ delay: 34 })
      }
      if (url.includes(encodeURIComponent(jpStuck)) && url.includes('/delay')) {
        init?.signal?.addEventListener('abort', () => resolveStuckProbe(jsonResponse({}, 499)))
        return stuckProbe
      }
      if (url === '/api/node-select' && init?.method === 'POST') {
        nodeSelect = 'manual'
        return jsonResponse({
          success: true,
          data: {
            requested: 'manual',
            node_select: 'manual',
            changed: true,
            runtime_updated: true,
            started: false,
            reloaded: true,
            restarted: false,
          },
        })
      }
      if (url === '/api/proxy/switch' && init?.method === 'POST') {
        const body = JSON.parse(String(init.body))
        currentExit = body.name
        return jsonResponse({
          success: true,
          data: { persisted: true },
        })
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    rs.stubGlobal('fetch', fetchMock)
    stubMatchMedia()
    const user = userEvent.setup()
    const { container } = render(() => <App />)

    const matrix = await screen.findByRole('group', { name: '候选节点列表' })
    expect(within(matrix).getByText('Hy2')).toBeInTheDocument()
    expect(within(matrix).getByText('1.5x')).toBeInTheDocument()
    expect(within(matrix).getByText('VLESS')).toBeInTheDocument()

    // 1. Independent single-node probe does NOT switch strategy or exit node
    await user.click(within(matrix).getByRole('button', { name: '测速节点 大阪极速 02' }))
    expect(await within(matrix).findByText('34 ms')).toBeInTheDocument()
    expect(fetchMock.mock.calls.some(([u]) => u === '/api/node-select' || u === '/api/proxy/switch')).toBe(false)

    // 2. Batch speed test shows progress & cancel button; stopping preserves finished probes
    await user.click(screen.getByRole('button', { name: '批量测速' }))
    const stopBtn = await screen.findByRole('button', { name: '停止测速' })
    await waitFor(() => {
      expect(within(matrix).getByText('165 ms')).toBeInTheDocument()
    })
    await user.click(stopBtn)
    expect(await screen.findByRole('button', { name: '批量测速' })).toBeInTheDocument()

    // 3. Sort by latency places 34 ms node ahead of 165 ms node and untested node
    const sortGroup = screen.getByRole('group', { name: '节点排序方式' })
    await user.click(within(sortGroup).getByRole('button', { name: /按延迟/ }))
    const cardNames = Array.from(container.querySelectorAll('.node-card-name')).map((el) => el.textContent)
    expect(cardNames).toEqual(['大阪极速 02', '东京标准 01 [1.5x]', '名古屋专线 03'])

    // 4. Clicking a node card body while in fastest_jp mode switches to manual + locks that node in 1 step
    const selectFastBtn = within(matrix).getByRole('button', { name: /大阪极速 02.*VLESS/ })
    await user.click(selectFastBtn)
    await waitFor(() => {
      const posts = fetchMock.mock.calls.filter(([, options]) => options?.method === 'POST')
      expect(posts.map(([u]) => u)).toEqual(['/api/node-select', '/api/proxy/switch'])
    })
  })
})
