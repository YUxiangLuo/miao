import { render, screen, within } from '@testing-library/react'
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
    expect(screen.getByText('2 条链接')).toBeInTheDocument()
    expect(screen.getByText('2 个站点 · 3 条链接')).toBeInTheDocument()
    expect(screen.getByText('Match')).toBeInTheDocument()
    expect(screen.getByText('RuleSet : chinasite')).toBeInTheDocument()
    expect(screen.getAllByText('api.github.com')).toHaveLength(1)
  })

  it('shows merged stat cards for speed and cumulative traffic', () => {
    renderModal([
      connection({ id: 'a', downloadSpeed: 2048, uploadSpeed: 1024, download: 2048, upload: 1024 }),
    ])

    expect(screen.getByText('站点')).toBeInTheDocument()
    expect(screen.getByText('实时速度')).toBeInTheDocument()
    expect(screen.getByText('累计流量')).toBeInTheDocument()
    // 合并后的速度卡与累计卡各自包含上下行两个数值
    expect(screen.getAllByText('2.0 KB/s')).not.toHaveLength(0)
    expect(screen.getAllByText('2.0 KB')).not.toHaveLength(0)
    expect(screen.getAllByText('1.0 KB')).not.toHaveLength(0)
  })

  it('aggregates cumulative traffic per site on the card', () => {
    renderModal([
      connection({ id: 'a', download: 2048, upload: 512 }),
      connection({ id: 'b', download: 1024, upload: 512 }),
    ])

    const head = screen.getByRole('button', { name: 'api.github.com 链接详情' })
    expect(within(head).getByText('累计')).toBeInTheDocument()
    expect(within(head).getByText('3.0 KB')).toBeInTheDocument()
    expect(within(head).getByText('1.0 KB')).toBeInTheDocument()
  })

  it('shows a friendly label for the final fallback rule', () => {
    renderModal([connection({ rule: 'final' })])

    expect(screen.getByText('兜底规则')).toBeInTheDocument()
    expect(screen.queryByText('final')).not.toBeInTheDocument()
  })

  it('expands a card to reveal per-connection details', async () => {
    const user = userEvent.setup()
    renderModal([
      connection({
        id: 'a',
        rule: 'Match',
        download: 2048,
        upload: 1024,
        start: '2020-01-01T00:00:00Z',
      }),
    ])

    const toggle = screen.getByRole('button', { name: 'api.github.com 链接详情' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getAllByText('Match')).toHaveLength(1)

    await user.click(toggle)
    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    // 卡片副标题与详情行各出现一次规则名
    expect(screen.getAllByText('Match')).toHaveLength(2)
    expect(screen.getByText('TCP/443')).toBeInTheDocument()
    expect(screen.getAllByText('proxy')).toHaveLength(2)
    expect(screen.getByTitle('建立于 2020-01-01T00:00:00Z')).toBeInTheDocument()
    expect(screen.getAllByText('2.0 KB')).not.toHaveLength(0)

    await user.click(toggle)
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getAllByText('Match')).toHaveLength(1)
  })

  it('keeps only one card expanded at a time', async () => {
    const user = userEvent.setup()
    renderModal([
      connection({ id: 'a', metadata: { host: 'alpha.dev' } }),
      connection({ id: 'b', metadata: { host: 'beta.dev' } }),
    ])

    const alpha = screen.getByRole('button', { name: 'alpha.dev 链接详情' })
    const beta = screen.getByRole('button', { name: 'beta.dev 链接详情' })

    await user.click(alpha)
    expect(alpha).toHaveAttribute('aria-expanded', 'true')

    await user.click(beta)
    expect(beta).toHaveAttribute('aria-expanded', 'true')
    expect(alpha).toHaveAttribute('aria-expanded', 'false')
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

    await user.type(screen.getByRole('searchbox', { name: '搜索链接' }), 'bilibili')
    expect(screen.getByText('www.bilibili.com')).toBeInTheDocument()
    expect(screen.queryByText('api.github.com')).not.toBeInTheDocument()
    expect(screen.getByText('1 / 2')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '清除搜索' }))
    expect(screen.getByText('api.github.com')).toBeInTheDocument()
    // 未筛选时不再重复显示站点计数
    expect(screen.queryByText('2 个站点')).not.toBeInTheDocument()
    expect(screen.queryByText('2 / 2')).not.toBeInTheDocument()
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

  it('sorts cards by combined speed (fixed order, no sort pills)', () => {
    renderModal([
      connection({ id: 'a', downloadSpeed: 10, uploadSpeed: 0, metadata: { host: 'slow.dev' } }),
      connection({ id: 'b', downloadSpeed: 0, uploadSpeed: 500, metadata: { host: 'fast.dev' } }),
    ])

    // 排序选择器已移除：固定按上下行合计速度降序
    expect(screen.queryByRole('button', { name: '速度' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '域名' })).not.toBeInTheDocument()
    const domains = screen.getAllByText(/\.dev$/).map((node) => node.textContent)
    expect(domains).toEqual(['fast.dev', 'slow.dev'])
  })
})
