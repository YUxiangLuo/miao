import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import App from './App'
import { RequestInterruptedError } from './hooks/request'
import { nodeMock, ruleMock, statusMock, subMock, subscriptionRefreshMock } from './testFixtures'

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

  it.each([
    ['nodes', '节点'], ['subs', '订阅'], ['rules', '规则'],
  ])('keeps %s deletion locked through readback and distinguishes deletion from readback failure', async (resource, label) => {
    const endpoint = `/api/${resource}`
    let finishDelete!: (response: ReturnType<typeof jsonResponse>) => void
    let finishRead!: (response: ReturnType<typeof jsonResponse>) => void
    let deleting = false
    const fetch = rs.fn(async (input, init?: RequestInit) => {
      const url = String(input)
      if (url === endpoint && init?.method === 'DELETE') {
        deleting = true
        return new Promise<ReturnType<typeof jsonResponse>>(resolve => { finishDelete = resolve })
      }
      if (url === endpoint && deleting) return new Promise<ReturnType<typeof jsonResponse>>(resolve => { finishRead = resolve })
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/nodes') return jsonResponse({ success: true, data: [nodeMock()] })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/rules') return jsonResponse({ success: true, data: [ruleMock({ raw: '{"domain":"example.com","outbound":"direct"}' })] })
      if (url === '/api/version') return jsonResponse({ success: true, data: { current: '0.48.1', latest: null, has_update: false } })
      throw new Error(`Unexpected request: ${url}`)
    })
    rs.stubGlobal('fetch', fetch)
    stubMatchMedia()
    const user = userEvent.setup()
    render(<App />)
    const button = await screen.findByRole('button', { name: new RegExp(`^删除${label} `) })
    await user.click(button)
    await user.click(screen.getByRole('button', { name: '确认' }))
    expect(button).toBeDisabled()
    expect(screen.getByText(`正在删除${label}并同步列表，请稍候…`)).toBeInTheDocument()
    await user.click(button)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    await act(async () => { finishDelete(jsonResponse({ success: true })) })
    await waitFor(() => expect(finishRead).toBeDefined())
    expect(button).toBeDisabled()
    await act(async () => { finishRead(jsonResponse({ success: false, message: '读取超时' }, 503)) })
    expect(await screen.findByText(`${label}已删除；列表尚未同步，请重新读取列表，勿重复删除。`)).toBeInTheDocument()
    expect(button).toBeEnabled()
    expect(fetch.mock.calls.filter(([, init]) => init?.method === 'DELETE')).toHaveLength(1)
  })

  it('unlocks deletion after a rejected request and allows an explicit retry', async () => {
    let attempts = 0
    rs.stubGlobal('fetch', rs.fn(async (input, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/nodes' && init?.method === 'DELETE') {
        attempts++
        return attempts === 1 ? jsonResponse({ success: false, message: '删除失败，请重试' }, 500) : jsonResponse({ success: true })
      }
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/nodes') return jsonResponse({ success: true, data: attempts < 2 ? [nodeMock()] : [] })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: { current: '0.48.1', latest: null, has_update: false } })
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    const user = userEvent.setup()
    render(<App />)
    await user.click(await screen.findByRole('button', { name: /^删除节点 / }))
    await user.click(screen.getByRole('button', { name: '确认' }))
    expect(await screen.findByText('删除失败，请重试')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /^删除节点 / })).toBeEnabled()
    await user.click(screen.getByRole('button', { name: /^删除节点 / }))
    await user.click(screen.getByRole('button', { name: '确认' }))
    expect(await screen.findByText('节点已删除')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /^删除节点 / })).not.toBeInTheDocument()
    expect(attempts).toBe(2)
  })

  it.each([
    ['/api/nodes', '手动节点', '暂无手动节点'],
    ['/api/subs', '订阅列表', '暂无订阅'],
    ['/api/rules', '自定义规则', '暂无自定义规则'],
  ])('distinguishes a failed list from an empty configuration and retries only the read: %s', async (endpoint, label, empty) => {
    let failed = true
    const fetch = rs.fn(async (input) => {
      const url = String(input)
      if (url === endpoint && failed) return jsonResponse({ success: false, message: '临时读取错误' }, 503)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs' || url === '/api/nodes' || url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: { current: '0.48.1', latest: null, has_update: false } })
      throw new Error(`Unexpected request: ${url}`)
    })
    rs.stubGlobal('fetch', fetch)
    stubMatchMedia()
    const user = userEvent.setup()
    render(<App />)
    expect(await screen.findByText(`${label}读取失败`)).toBeInTheDocument()
    expect(screen.queryByText(empty)).not.toBeInTheDocument()
    expect(screen.queryByText('添加订阅链接或手动节点以开始使用')).not.toBeInTheDocument()
    failed = false
    await user.click(screen.getByRole('button', { name: `重新读取${label}` }))
    expect(await screen.findByText('添加订阅链接或手动节点以开始使用')).toBeInTheDocument()
    expect(screen.queryByText(`${label}读取失败`)).not.toBeInTheDocument()
    expect(fetch.mock.calls.filter(([url]) => url === endpoint)).toHaveLength(2)
  })

  it.each([
    [subscriptionRefreshMock({ fetch_succeeded: false, refreshed: false }), '订阅获取失败，已保留原配置。请查看订阅错误详情。'],
    [subscriptionRefreshMock({ warning: '部分订阅失败，使用缓存' }), '订阅刷新有告警：部分订阅失败，使用缓存'],
    [subscriptionRefreshMock({ report: { successful_sources: 2, failed_sources: 1, fresh_nodes: 7, cached_nodes: 3 } }), '订阅部分获取成功：成功 2 个，失败 1 个。请查看订阅详情。'],
    [subscriptionRefreshMock({ runtime_updated: true }), '订阅已获取，代理配置已更新'],
    [subscriptionRefreshMock(), '订阅已获取，代理运行状态未改变'],
    [null, '刷新请求已处理；后端未提供详细结果，请查看订阅状态。'],
  ])('reports the actual manual refresh result: %j', async (data, message) => {
    rs.stubGlobal('fetch', rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes' || url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: { current: '0.48.1', latest: null, has_update: false } })
      if (url === '/api/subs/refresh') return jsonResponse({ success: true, data })
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    const user = userEvent.setup()
    render(<App />)
    await user.click(await screen.findByRole('button', { name: '刷新订阅' }))
    await user.click(screen.getByRole('button', { name: '确认' }))
    expect(await screen.findByText(message)).toBeInTheDocument()
    expect(screen.queryByText('订阅已刷新')).not.toBeInTheDocument()
  })

  it('retains deployment results after closing the pane without closing a subsequently opened dialog', async () => {
    let finish!: (response: ReturnType<typeof jsonResponse>) => void
    rs.stubGlobal('fetch', rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs' || url === '/api/nodes' || url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: { current: '0.48.1', latest: null, has_update: false } })
      if (url === '/api/vps/deploy') return new Promise<ReturnType<typeof jsonResponse>>(resolve => { finish = resolve })
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    const user = userEvent.setup()
    render(<App />)
    await user.click(await screen.findByRole('button', { name: '手动添加节点' }))
    await user.click(screen.getByRole('button', { name: 'VPS 部署' }))
    await user.type(screen.getByLabelText('VPS IP 地址'), '203.0.113.10')
    await user.type(screen.getByLabelText('root 密码'), 'demo')
    await user.click(screen.getByRole('button', { name: '开始部署' }))
    await user.click(screen.getByRole('button', { name: '关闭节点对话框' }))
    expect(screen.getByText(/VPS 部署记录 · 等待部署结果/)).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '手动添加节点' }))
    await act(async () => { finish(jsonResponse({ success: true, message: '部署完成' })) })
    expect(screen.getByRole('dialog', { name: '添加节点' })).toBeInTheDocument()
    expect(screen.getByLabelText('节点分享链接')).toBeInTheDocument()
    expect(screen.getByText(/VPS 部署记录 · 已完成/)).toBeInTheDocument()
  })

  it('reports upgrade installation, then restores the button when only the old version returns', async () => {
    rs.useFakeTimers()
    let upgrades = 0
    let finishUpgrade!: (response: ReturnType<typeof jsonResponse>) => void
    rs.stubGlobal('fetch', rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes' || url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: {
        current: '0.48.1', latest: 'v0.49.0', has_update: true, download_url: null, upgrade_supported: true,
      } })
      if (url === '/api/upgrade') { upgrades++; return new Promise<ReturnType<typeof jsonResponse>>(resolve => { finishUpgrade = resolve }) }
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    render(<App />)
    await act(async () => { await rs.advanceTimersByTimeAsync(0) })
    fireEvent.click(screen.getByRole('button', { name: /0\.48\.1\s*可更新至 v0\.49\.0/ }))
    fireEvent.click(screen.getByRole('button', { name: '确认' }))
    await act(async () => { await rs.advanceTimersByTimeAsync(0) })
    expect(screen.getByText(/正在请求升级 0.48.1 → v0.49.0/)).toBeInTheDocument()
    await act(async () => { finishUpgrade(jsonResponse({ success: true, data: 'v0.49.0' })) })
    expect(screen.getByText(/后端已完成安装，正在确认 v0.49.0 启动/)).toBeInTheDocument()
    expect(screen.queryByText('更新成功，等待服务重启…')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /0\.48\.1\s*可更新至 v0\.49\.0/ })).toBeDisabled()
    await act(async () => { await rs.advanceTimersByTimeAsync(30_000) })
    expect(screen.getByText(/可能尚未重启或已回滚/)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /0\.48\.1\s*可更新至 v0\.49\.0/ })).not.toBeDisabled()
    expect(upgrades).toBe(1)
  })

  it.each([
    [new TypeError('Failed to fetch'), /升级请求连接中断/, /可能尚未重启或已回滚/],
    [new RequestInterruptedError('timeout'), /升级请求连接中断/, /可能尚未重启或已回滚/],
    ['v0.50.0', /正在确认 v0.50.0 启动/, /当前版本为 0.49.0/],
  ])('keeps verifying uncertain requests and uses the installed target: %s', async (outcome, progress, error) => {
    rs.useFakeTimers()
    let upgrades = 0
    rs.stubGlobal('fetch', rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes' || url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: {
        current: upgrades && typeof outcome === 'string' ? '0.49.0' : '0.48.1', latest: 'v0.49.0', has_update: true,
      } })
      if (url === '/api/upgrade') {
        upgrades++
        if (outcome instanceof Error) throw outcome
        return jsonResponse({ success: true, data: outcome })
      }
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    render(<App />)
    await act(async () => { await rs.advanceTimersByTimeAsync(0) })
    fireEvent.click(screen.getByRole('button', { name: /可更新至 v0\.49\.0/ }))
    fireEvent.click(screen.getByRole('button', { name: '确认' }))
    await act(async () => { await rs.advanceTimersByTimeAsync(4000) })
    expect(screen.getByText(progress)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /可更新至 v0\.49\.0/ })).toBeDisabled()
    await act(async () => { await rs.advanceTimersByTimeAsync(26_000) })
    expect(screen.getByText(error)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /可更新至 v0\.49\.0/ })).not.toBeDisabled()
    expect(upgrades).toBe(1)
  })

  it.each([
    [true, '后端已是最新版本，无需升级'],
    [false, '下载校验失败'],
  ])('does not wait for restart after a no-op or rejected upgrade: %s', async (success, message) => {
    rs.useFakeTimers()
    rs.stubGlobal('fetch', rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs') return jsonResponse({ success: true, data: [subMock()] })
      if (url === '/api/nodes' || url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: { current: '0.48.1', latest: 'v0.49.0', has_update: true } })
      if (url === '/api/upgrade') return jsonResponse({ success, message: success ? 'Already up to date' : message })
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    render(<App />)
    await act(async () => { await rs.advanceTimersByTimeAsync(0) })
    fireEvent.click(screen.getByRole('button', { name: /可更新至 v0\.49\.0/ }))
    fireEvent.click(screen.getByRole('button', { name: '确认' }))
    await act(async () => { await rs.advanceTimersByTimeAsync(0) })
    expect(screen.getByText(message)).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /可更新至 v0\.49\.0/ })).not.toBeDisabled()
    expect(screen.queryByText(/正在确认.*启动|升级请求连接中断/)).not.toBeInTheDocument()
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
      if (url === '/api/nodes' || url === '/api/rules') {
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
    expect(await screen.findByText('订阅地址已保存，请查看节点获取状态')).toBeInTheDocument()
    expect(await screen.findByText('订阅管理')).toBeInTheDocument()
  })

  it.each([
    [{ added: 1, skipped: 2 }, '已新增 1 条订阅地址，跳过 2 条已有订阅。请查看节点获取状态。'],
    [{ added: 0, skipped: 3 }, '已新增 0 条订阅地址，跳过 3 条已有订阅。请查看节点获取状态。'],
    [null, '订阅导入请求已完成，请查看订阅列表和节点获取状态。'],
  ])('reports import counts without guessing from the selection: %j', async (data, message) => {
    rs.stubGlobal('fetch', rs.fn(async (input) => {
      const url = String(input)
      if (url === '/api/status') return jsonResponse({ success: true, data: statusMock() })
      if (url === '/api/subs' || url === '/api/nodes' || url === '/api/rules') return jsonResponse({ success: true, data: [] })
      if (url === '/api/version') return jsonResponse({ success: true, data: { current: '0.48.1', latest: null, has_update: false } })
      if (url === '/api/import/clash-verge') return jsonResponse({ success: true, data: { found: true, items: [1, 2, 3].map(i => ({ name: `订阅 ${i}`, url: `https://example.com/sub${i}`, already_added: false })) } })
      if (url === '/api/subs/batch') return jsonResponse({ success: true, data })
      throw new Error(`Unexpected request: ${url}`)
    }))
    stubMatchMedia()
    const user = userEvent.setup()
    render(<App />)
    await user.click(await screen.findByRole('button', { name: '从 Clash Verge Rev 导入' }))
    await user.click(await screen.findByRole('button', { name: '导入 3 条' }))
    expect(await screen.findByText(message)).toBeInTheDocument()
    expect(screen.queryByText('已导入 3 条订阅')).not.toBeInTheDocument()
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
    const measured = new Date(2026, 8, 22, 13, 24, 56).toISOString()
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
            'node-a': { type: 'Hysteria2', name: 'node-a', history: [{ time: measured, delay: 96 }, { time: '2026-01-01T00:00:00Z', delay: 300 }] },
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
      expect(screen.getByText('测于 09/22 13:24:56')).toBeInTheDocument()
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
    expect(screen.getByRole('status')).toHaveTextContent('无法连接后端，正在自动重试…')
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

  it.each([
    { phase: 'failed' as const, source: 'node', label: '重新启动' },
    { phase: 'stopped' as const, source: 'node', label: '启动代理' },
    { phase: 'stopped' as const, source: 'subscription', label: '启动代理' },
  ])('starts a $phase proxy service with a configured $source', async ({ phase, source, label }) => {
    let started = false
    const fetchMock = rs.fn(async (input, options = {}) => {
      const url = String(input)
      if (url === '/api/status') {
        return jsonResponse({
          success: true,
          data: statusMock({
            running: started,
            ready: started,
            phase: started ? 'ready' : phase,
          }),
        })
      }
      if (url === '/api/nodes') {
        return jsonResponse({
          success: true,
          data: source === 'node' ? [nodeMock({ tag: 'node-a' })] : [],
        })
      }
      if (url === '/api/subs' || url === '/api/rules') {
        return jsonResponse({ success: true, data: url === '/api/subs' && source === 'subscription' ? [subMock()] : [] })
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

    const retry = await screen.findByRole('button', { name: label })
    expect(fetchMock).not.toHaveBeenCalledWith('/api/service/start', expect.anything())
    await user.click(retry)

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/service/start', expect.objectContaining({ method: 'POST' }))
    })
    expect(await screen.findByText('代理服务已重新启动')).toBeInTheDocument()
    await waitFor(() => expect(screen.queryByRole('button', { name: label })).not.toBeInTheDocument())
  })
})
