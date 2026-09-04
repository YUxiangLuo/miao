import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import { SubsCard } from './SubsCard'
import { subMock, subNodeMock } from '../testFixtures'

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
        data: [{ url: subs[0].url, nodes: [subNodeMock({ name: '香港 01' })] }],
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

  it('keeps the node count non-clickable for failed subscriptions', () => {
    renderCard({
      subs: [subMock({ success: false, node_count: 0, state: 'failed', error: 'boom' })],
    })

    expect(screen.queryByRole('button', { name: /个节点/ })).not.toBeInTheDocument()
    expect(screen.getByText('boom')).toBeInTheDocument()
  })
})

describe('SubsCard header actions', () => {
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
