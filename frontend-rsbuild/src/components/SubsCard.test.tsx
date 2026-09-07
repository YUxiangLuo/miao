import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { SubsCard } from './SubsCard'
import { statusMock, subMock, subNodeMock, subNodesInfoMock } from '../testFixtures'

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
  it('shows background retry independently and leaves manual refresh available', () => {
    renderCard({ refreshStatus: {
      ...statusMock().subscription_refresh, phase: 'waiting', retry_in_secs: 1800,
    } })
    expect(screen.getByRole('status')).toHaveTextContent('后台等待重试（约 30 分钟后），可手动刷新')
    expect(screen.getByRole('button', { name: '刷新订阅' })).toBeEnabled()
  })

  it('places the refresh button next to the title and add at the far right', async () => {
    const user = userEvent.setup()
    const { props } = renderCard()

    const header = document.querySelector('.section-header') as HTMLElement
    const titleWrap = header.querySelector('.section-title-wrap') as HTMLElement
    // 刷新按钮紧跟标题（在 title-wrap 内），添加按钮在标题栏最右（title-wrap 外）
    expect(within(titleWrap).getByRole('button', { name: '刷新订阅' })).toBeInTheDocument()
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
