import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ConnectionsModal } from './ConnectionsModal.jsx'

function connection({ metadata, ...overrides } = {}) {
  return {
    id: 'connection-1',
    upload: 0,
    download: 0,
    uploadSpeed: 0,
    downloadSpeed: 0,
    chains: ['proxy'],
    rule: 'Match',
    ...overrides,
    metadata: {
      host: 'api.github.com',
      destinationPort: 443,
      network: 'tcp',
      sourceIP: '127.0.0.1',
      ...metadata,
    },
  }
}

function renderModal(connections, props = {}) {
  return render(
    <ConnectionsModal
      open
      status={{ running: true }}
      data={{ uploadTotal: 0, downloadTotal: 0, connections }}
      loading={false}
      error=""
      onClose={vi.fn()}
      onRefresh={vi.fn()}
      onCloseConnection={vi.fn()}
      showToast={vi.fn()}
      {...props}
    />,
  )
}

describe('ConnectionsModal cards', () => {
  it('groups sockets by host and shows the domain and rule', () => {
    renderModal([
      connection({ id: 'a', downloadSpeed: 1200 }),
      connection({ id: 'b', downloadSpeed: 800 }),
      connection({
        id: 'c',
        rule: 'RuleSet',
        rulePayload: 'chinasite',
        chains: ['direct'],
        metadata: { host: 'www.bilibili.com' },
      }),
    ])

    expect(screen.getByText('api.github.com')).toBeInTheDocument()
    expect(screen.getByText('www.bilibili.com')).toBeInTheDocument()
    expect(screen.getByText('2 条')).toBeInTheDocument()
    expect(screen.getByText('Match')).toBeInTheDocument()
    expect(screen.getByText('RuleSet : chinasite')).toBeInTheDocument()
    expect(screen.getAllByText('api.github.com')).toHaveLength(1)
  })

  it('uses a curated mark for known sites and a letter for unknown hosts', () => {
    renderModal([
      connection({ id: 'github' }),
      connection({
        id: 'unknown',
        metadata: { host: 'obscure-lab.internal' },
      }),
    ])

    expect(document.querySelector('[data-site="github"]')).not.toBeNull()
    expect(document.querySelector('[data-site="letter"]')).toHaveTextContent('O')
  })

  it('does not turn a card into a details control', async () => {
    const user = userEvent.setup()
    renderModal([connection()])

    expect(screen.queryByRole('button', { name: /查看连接/ })).not.toBeInTheDocument()
    expect(screen.queryByText('连接详情')).not.toBeInTheDocument()

    await user.click(screen.getByText('api.github.com'))
    expect(screen.queryByText('连接详情')).not.toBeInTheDocument()
  })

  it('closes every connection for a host from the card action', async () => {
    const user = userEvent.setup()
    const onCloseConnection = vi.fn().mockResolvedValue()
    renderModal(
      [
        connection({ id: 'a' }),
        connection({ id: 'b' }),
      ],
      { onCloseConnection },
    )

    await user.click(screen.getByRole('button', { name: '关闭 api.github.com 的连接' }))
    expect(onCloseConnection).toHaveBeenCalledTimes(2)
    expect(onCloseConnection).toHaveBeenNthCalledWith(1, 'a')
    expect(onCloseConnection).toHaveBeenNthCalledWith(2, 'b')
  })

  it('filters cards by domain or rule', async () => {
    const user = userEvent.setup()
    renderModal([
      connection({ id: 'a' }),
      connection({
        id: 'c',
        rule: 'RuleSet',
        rulePayload: 'chinasite',
        metadata: { host: 'www.bilibili.com' },
      }),
    ])

    await user.type(screen.getByRole('searchbox', { name: '搜索连接' }), 'bilibili')
    expect(screen.getByText('www.bilibili.com')).toBeInTheDocument()
    expect(screen.queryByText('api.github.com')).not.toBeInTheDocument()
    expect(screen.getByText('1 / 2')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '清除搜索' }))
    expect(screen.getByText('api.github.com')).toBeInTheDocument()
    expect(screen.getByText('2 个站点')).toBeInTheDocument()
  })

  it('filters cards by proxy or direct outbound', async () => {
    const user = userEvent.setup()
    renderModal([
      connection({ id: 'a' }),
      connection({
        id: 'c',
        rule: 'RuleSet',
        rulePayload: 'chinasite',
        chains: ['direct'],
        metadata: { host: 'www.bilibili.com' },
      }),
    ])

    await user.click(screen.getByRole('button', { name: /直连/ }))
    expect(screen.getByText('www.bilibili.com')).toBeInTheDocument()
    expect(screen.queryByText('api.github.com')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /代理/ }))
    expect(screen.getByText('api.github.com')).toBeInTheDocument()
    expect(screen.queryByText('www.bilibili.com')).not.toBeInTheDocument()
  })

  it('sorts cards by domain', async () => {
    const user = userEvent.setup()
    renderModal([
      connection({ id: 'a', downloadSpeed: 900, metadata: { host: 'zeta.dev' } }),
      connection({ id: 'b', downloadSpeed: 100, metadata: { host: 'alpha.dev' } }),
    ])

    await user.click(screen.getByRole('button', { name: '域名' }))
    const domains = screen.getAllByText(/\.dev$/).map((node) => node.textContent)
    expect(domains).toEqual(['alpha.dev', 'zeta.dev'])
  })
})
