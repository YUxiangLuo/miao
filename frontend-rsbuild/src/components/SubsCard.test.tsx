import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { SubsCard } from './SubsCard'
import {
  scheduledRefreshStatusMock, statusMock, subMock, subNodeMock, subNodesInfoMock,
} from '../testFixtures'

const subs = [
  subMock({ url: 'https://example.com/subscription-token-abcdef', node_count: 42 }),
]

function renderCard(overrides = {}) {
  const props = {
    subs,
    pendingActions: new Set<string>(),
    onAddSub: rs.fn().mockResolvedValue(true),
    onDeleteSub: rs.fn(),
    onRefreshSubs: rs.fn(),
    onToggleNodeDisabled: rs.fn().mockResolvedValue(true),
    onSaveSchedule: rs.fn().mockResolvedValue(true),
    isInitializing: false,
    ...overrides,
  }
  return { ...render(<SubsCard {...props} />), props }
}

describe('SubsCard subscription detail entry', () => {
  afterEach(() => {
    rs.unstubAllGlobals()
  })

  it('opens the detail modal from the clickable node count', async () => {
    const user = userEvent.setup()
    rs.stubGlobal('fetch', rs.fn(async () => ({
      ok: true,
      json: async () => ({
        success: true,
        message: 'ok',
        data: [subNodesInfoMock({ url: subs[0].url, nodes: [subNodeMock({ name: '香港 01' })] })],
      }),
    })))
    renderCard()

    await user.click(screen.getByRole('button', { name: /42 个节点/ }))

    expect(await screen.findByRole('dialog')).toBeInTheDocument()
    expect(await screen.findByText('香港 01')).toBeInTheDocument()
  })

  it('shows the disabled count next to the node count', () => {
    renderCard({ subs: [subMock({ ...subs[0], disabled_count: 2 })] })

    expect(screen.getByRole('button', { name: /42 个节点 · 禁用 2/ })).toBeInTheDocument()
  })

  it('shows successful empty responses without a failure badge', () => {
    renderCard({ subs: [subMock({ success: true, state: 'ready', node_count: 0 })] })
    expect(screen.getByText('获取成功，暂无代理节点')).toBeInTheDocument()
    expect(screen.queryByText('获取失败')).not.toBeInTheDocument()
    expect(document.querySelector('.status-icon-badge.error')).not.toBeInTheDocument()
  })

  it('lets the user manage cached nodes while showing the refresh failure', async () => {
    const user = userEvent.setup()
    const sub = subMock({ success: false, node_count: 2, state: 'failed', error: 'Request timeout' })
    let disabled = false
    rs.stubGlobal('fetch', rs.fn(async () => ({
      ok: true,
      json: async () => ({
        success: true,
        message: 'ok',
        data: [subNodesInfoMock({ url: sub.url, nodes: [
          subNodeMock({ name: '缓存节点', disabled }),
          subNodeMock({ name: '备用节点' }),
        ] })],
      }),
    })))
    const onToggleNodeDisabled = rs.fn(async (_sub: string, _name: string, next: boolean) => {
      disabled = next
      return true
    })
    renderCard({ subs: [sub], onToggleNodeDisabled })

    expect(screen.getByText('Request timeout')).toBeInTheDocument()
    expect(screen.getByText('保留上次获取的 2 个缓存节点，不代表本次获取成功')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '2 个节点' }))
    await user.click(await screen.findByRole('switch', { name: '禁用节点 缓存节点' }))
    expect(onToggleNodeDisabled).toHaveBeenCalledWith(sub.url, '缓存节点', true)
    expect(await screen.findByRole('switch', { name: '启用节点 缓存节点' })).toHaveAttribute('aria-checked', 'false')
  })

  it('lets the user clear stale disabled entries after a successful empty response', async () => {
    const user = userEvent.setup()
    const sub = subMock({ node_count: 0 })
    let stale = ['已移除节点']
    rs.stubGlobal('fetch', rs.fn(async () => ({
      ok: true,
      json: async () => ({
        success: true,
        message: 'ok',
        data: [subNodesInfoMock({ url: sub.url, stale_disabled: stale })],
      }),
    })))
    const onToggleNodeDisabled = rs.fn(async () => {
      stale = []
      return true
    })
    renderCard({ subs: [sub], onToggleNodeDisabled })

    await user.click(screen.getByRole('button', { name: '查看节点' }))
    await user.click(await screen.findByRole('button', { name: '移除失效禁用 已移除节点' }))
    expect(onToggleNodeDisabled).toHaveBeenCalledWith(sub.url, '已移除节点', false)
    await waitFor(() => expect(screen.queryByText('已移除节点')).not.toBeInTheDocument())
  })
})

describe('SubsCard header actions', () => {
  it('distinguishes partial fetch results from cached nodes without claiming activation', () => {
    renderCard({ refreshStatus: {
      phase: 'completed', outcome: 'partial_failure', retry_in_secs: null,
      report: { successful_sources: 2, failed_sources: 1, fresh_nodes: 7, cached_nodes: 3 },
    } })
    expect(screen.getByRole('status')).toHaveTextContent('成功 2 个订阅，失败 1 个订阅；本次获取 7 个节点，保留 3 个缓存节点。获取结果不代表配置已生效。')
  })

  it('hides the previous fetch result while a manual refresh is pending', () => {
    renderCard({ pendingActions: new Set(['refreshSubs']), refreshStatus: {
      ...statusMock().subscription_refresh, phase: 'completed', outcome: 'success',
    } })
    expect(screen.getByRole('status')).toHaveTextContent('正在获取订阅并检查配置')
    expect(screen.queryByText(/最近一次获取/)).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '刷新订阅' })).toBeDisabled()
  })

  it('shows background retry independently and leaves manual refresh available', () => {
    renderCard({ refreshStatus: {
      ...statusMock().subscription_refresh, phase: 'waiting', retry_in_secs: 1800,
    } })
    expect(screen.getByRole('status')).toHaveTextContent('后台等待重试（约 30 分钟后），可手动刷新')
    expect(screen.getByRole('button', { name: '刷新订阅' })).toBeEnabled()
  })

  it('places the refresh and schedule icon buttons next to the title and add at the far right', async () => {
    const user = userEvent.setup()
    const { props } = renderCard()

    const header = document.querySelector('.section-header') as HTMLElement
    const titleWrap = header.querySelector('.section-title-wrap') as HTMLElement
    // 刷新与定时刷新都是标题旁的 icon 按钮（在 title-wrap 内），添加按钮在标题栏最右（title-wrap 外）
    expect(within(titleWrap).getByRole('button', { name: '刷新订阅' })).toBeInTheDocument()
    expect(within(titleWrap).getByRole('button', { name: '定时刷新' })).toBeInTheDocument()
    expect(within(titleWrap).queryByRole('button', { name: '添加' })).not.toBeInTheDocument()
    expect(within(header).getByRole('button', { name: '添加' })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '刷新订阅' }))
    expect(props.onRefreshSubs).toHaveBeenCalledTimes(1)
  })

  it('shows no inline add input in the card body', () => {
    renderCard()
    expect(screen.queryByPlaceholderText('粘贴订阅链接...')).not.toBeInTheDocument()
  })
})

describe('SubsCard add modal', () => {
  it('blocks repeated Enter submissions and keeps a reopened dialog when an older request completes', async () => {
    const user = userEvent.setup()
    let finish!: (success: boolean) => void
    const onAddSub = rs.fn(() => new Promise<boolean>(resolve => { finish = resolve }))
    renderCard({ onAddSub })
    await user.click(screen.getByRole('button', { name: '添加' }))
    const input = screen.getByLabelText('订阅链接')
    await user.type(input, 'https://example.com/sub')
    act(() => {
      fireEvent.keyDown(input, { key: 'Enter' })
      fireEvent.keyDown(input, { key: 'Enter' })
    })
    expect(onAddSub).toHaveBeenCalledTimes(1)
    expect(input).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent('关闭窗口不会取消请求')
    await user.click(screen.getByRole('button', { name: '关闭窗口' }))
    await user.click(screen.getByRole('button', { name: '添加' }))
    await act(async () => { finish(true) })
    expect(screen.getByRole('dialog', { name: '添加订阅' })).toBeInTheDocument()
    expect(screen.getByLabelText('订阅链接')).toHaveValue('')
    expect(screen.getByLabelText('订阅链接')).toBeEnabled()
  })

  it('opens the modal from the header button and submits the trimmed url', async () => {
    const user = userEvent.setup()
    const { props } = renderCard()

    await user.click(screen.getByRole('button', { name: '添加' }))
    const dialog = screen.getByRole('dialog', { name: '添加订阅' })
    const input = within(dialog).getByLabelText('订阅链接')
    expect(input).toHaveFocus()

    await user.type(input, '  https://example.com/sub  ')
    await user.click(within(dialog).getByRole('button', { name: '添加' }))

    expect(props.onAddSub).toHaveBeenCalledWith('https://example.com/sub')
    // 成功后弹窗关闭
    expect(screen.queryByRole('dialog', { name: '添加订阅' })).not.toBeInTheDocument()
  })

  it('keeps the modal open when the submit fails', async () => {
    const user = userEvent.setup()
    const { props } = renderCard({ onAddSub: rs.fn().mockResolvedValue(false) })

    await user.click(screen.getByRole('button', { name: '添加' }))
    await user.type(screen.getByLabelText('订阅链接'), 'https://example.com/sub')
    await user.click(screen.getByRole('dialog', { name: '添加订阅' }).querySelector('.modal-actions button:last-child') as HTMLElement)

    expect(props.onAddSub).toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: '添加订阅' })).toBeInTheDocument()
    expect(screen.getByLabelText('订阅链接')).toHaveValue('https://example.com/sub')
    expect(screen.getByLabelText('订阅链接')).toBeEnabled()
  })

  it('disables submit while the input is empty', async () => {
    const user = userEvent.setup()
    renderCard()

    await user.click(screen.getByRole('button', { name: '添加' }))
    const dialog = screen.getByRole('dialog', { name: '添加订阅' })
    const submit = within(dialog).getAllByRole('button').find((b) => b.textContent === '添加')!
    expect(submit).toBeDisabled()
  })
})

describe('SubsCard schedule modal', () => {
  afterEach(() => {
    rs.unstubAllGlobals()
  })

  it('preserves read failure details and retries without writing or showing default settings', async () => {
    const user = userEvent.setup()
    let finish!: (response: unknown) => void
    const fetch = rs.fn().mockRejectedValueOnce(new Error('连接超时：测试原因'))
      .mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
    rs.stubGlobal('fetch', fetch)
    const { props } = renderCard()
    await user.click(screen.getByRole('button', { name: '定时刷新' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('不代表定时刷新已关闭')
    expect(screen.getByRole('button', { name: '保存' })).toBeDisabled()
    expect(screen.queryByRole('switch', { name: '启用定时刷新' })).not.toBeInTheDocument()
    await user.click(screen.getByText('错误详情'))
    expect(screen.getByText('连接超时：测试原因')).toBeVisible()
    const retry = screen.getByRole('button', { name: '重新读取设置' })
    act(() => { fireEvent.click(retry); fireEvent.click(retry) })
    expect(fetch).toHaveBeenCalledTimes(2)
    expect(screen.getByRole('status')).toHaveTextContent('正在读取定时刷新设置')
    expect(screen.getByRole('button', { name: '保存' })).toBeDisabled()
    await act(async () => { finish({ ok: true, json: async () => ({ success: true, data: scheduledRefreshStatusMock({ enabled: true, times: ['18:37'] }) }) }) })
    expect(screen.getByLabelText('第 1 个执行时间')).toHaveValue('18:37')
    expect(screen.getByRole('switch', { name: '启用定时刷新' })).toHaveAttribute('aria-checked', 'true')
    expect(screen.getByRole('button', { name: '保存' })).toBeEnabled()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(props.onSaveSchedule).not.toHaveBeenCalled()
    for (const [url, options] of fetch.mock.calls) {
      expect(url).toBe('/api/scheduled-refresh')
      expect(options?.method).toBeUndefined()
    }
  })

  it('ignores an old retry result after closing and reopening', async () => {
    const user = userEvent.setup()
    let finish!: (response: unknown) => void
    rs.stubGlobal('fetch', rs.fn().mockRejectedValueOnce(new Error('读取失败'))
      .mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
      .mockResolvedValueOnce({ ok: true, json: async () => ({ success: true, data: scheduledRefreshStatusMock({ times: ['19:41'] }) }) }))
    renderCard()
    await user.click(screen.getByRole('button', { name: '定时刷新' }))
    await user.click(await screen.findByRole('button', { name: '重新读取设置' }))
    await user.click(screen.getByRole('button', { name: '关闭定时刷新对话框' }))
    await user.click(screen.getByRole('button', { name: '定时刷新' }))
    expect(await screen.findByDisplayValue('19:41')).toBeInTheDocument()
    await act(async () => { finish({ ok: true, json: async () => ({ success: true, data: scheduledRefreshStatusMock({ times: ['03:12'] }) }) }) })
    expect(screen.getByLabelText('第 1 个执行时间')).toHaveValue('19:41')
  })

  it('locks the submitted schedule, prevents duplicate saves, and leaves a reopened window open', async () => {
    const user = userEvent.setup()
    rs.stubGlobal('fetch', rs.fn(async () => ({ ok: true, json: async () => ({ success: true, data: scheduledRefreshStatusMock({ enabled: true, times: ['04:05'] }) }) })))
    let finish!: (saved: boolean) => void
    const onSaveSchedule = rs.fn(() => new Promise<boolean>(resolve => { finish = resolve }))
    renderCard({ onSaveSchedule })
    await user.click(screen.getByRole('button', { name: '定时刷新' }))
    const time = await screen.findByLabelText('第 1 个执行时间')
    fireEvent.change(time, { target: { value: '17:23' } })
    const save = screen.getByRole('button', { name: '保存' })
    act(() => { fireEvent.click(save); fireEvent.click(save) })
    expect(onSaveSchedule).toHaveBeenCalledExactlyOnceWith({ enabled: true, times: ['17:23'] })
    expect(time).toBeDisabled()
    expect(screen.getByRole('switch', { name: '启用定时刷新' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '添加时间' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '删除第 1 个执行时间' })).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent('关闭窗口不会取消请求')
    await user.click(screen.getByRole('button', { name: '关闭窗口' }))
    await user.click(screen.getByRole('button', { name: '定时刷新' }))
    expect(await screen.findByDisplayValue('04:05')).toBeInTheDocument()
    await act(async () => { finish(true) })
    expect(screen.getByRole('dialog', { name: '定时刷新' })).toBeInTheDocument()
    expect(screen.getByLabelText('第 1 个执行时间')).toBeEnabled()
    expect(screen.getByLabelText('第 1 个执行时间')).toHaveValue('04:05')
  })

  it('retains a failed schedule draft and allows retry', async () => {
    const user = userEvent.setup()
    rs.stubGlobal('fetch', rs.fn(async () => ({ ok: true, json: async () => ({ success: true, data: scheduledRefreshStatusMock({ enabled: true, times: ['04:05'] }) }) })))
    const onSaveSchedule = rs.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    renderCard({ onSaveSchedule })
    await user.click(screen.getByRole('button', { name: '定时刷新' }))
    fireEvent.change(await screen.findByLabelText('第 1 个执行时间'), { target: { value: '17:23' } })
    await user.click(screen.getByRole('button', { name: '保存' }))
    expect(screen.getByLabelText('第 1 个执行时间')).toHaveValue('17:23')
    expect(screen.getByLabelText('第 1 个执行时间')).toBeEnabled()
    expect(screen.getByRole('button', { name: '保存' })).toBeEnabled()
    await user.click(screen.getByRole('button', { name: '保存' }))
    expect(onSaveSchedule).toHaveBeenNthCalledWith(2, { enabled: true, times: ['17:23'] })
    expect(screen.queryByRole('dialog', { name: '定时刷新' })).not.toBeInTheDocument()
  })

  it('opens the schedule modal from the header button and saves settings', async () => {
    const user = userEvent.setup()
    rs.stubGlobal('fetch', rs.fn(async () => ({
      ok: true,
      json: async () => ({
        success: true,
        message: 'ok',
        data: scheduledRefreshStatusMock({ enabled: true, times: ['04:05'] }),
      }),
    })))
    const { props } = renderCard({ onSaveSchedule: rs.fn().mockResolvedValue(true) })

    await user.click(screen.getByRole('button', { name: '定时刷新' }))

    const dialog = await screen.findByRole('dialog', { name: '定时刷新' })
    expect(await within(dialog).findByDisplayValue('04:05')).toBeInTheDocument()

    await user.click(within(dialog).getByRole('button', { name: '保存' }))
    await waitFor(() => expect(props.onSaveSchedule).toHaveBeenCalledWith({
      enabled: true,
      times: ['04:05'],
    }))
    expect(screen.queryByRole('dialog', { name: '定时刷新' })).not.toBeInTheDocument()
  })
})
