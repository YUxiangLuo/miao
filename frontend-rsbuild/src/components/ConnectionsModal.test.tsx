import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { ConnectionsModal } from './ConnectionsModal'
import { connectionMock, statusMock } from '../testFixtures'
import type { EnrichedConnection } from '../types/clash'

function connection({ metadata, ...overrides }: Partial<EnrichedConnection> = {}) {
  return connectionMock({
    ...overrides,
    metadata: {
      host: 'api.github.com',
      destinationPort: '443',
      network: 'tcp',
      sourceIP: '127.0.0.1',
      ...metadata,
    },
  })
}

function renderModal(connections: EnrichedConnection[], props = {}) {
  return render(
    <ConnectionsModal
      open
      status={statusMock({ running: true })}
      data={{ uploadTotal: 0, downloadTotal: 0, connections }}
      loading={false}
      error=""
      onClose={rs.fn()}
      {...props}
    />,
  )
}

describe('ConnectionsModal connection list', () => {
  it('lists every connection as its own row, ungrouped', () => {
    renderModal([
      connection({ id: 'a' }),
      connection({ id: 'b' }),
      connection({ id: 'c', metadata: { host: 'api.github.com' } }),
    ])

    // 同域名的三条连接各占一行，不再聚合成站点卡
    expect(screen.getAllByText('api.github.com')).toHaveLength(3)
  })

  it('shows humanized rule, process, network and duration in the row subtitle', () => {
    renderModal([
      connection({
        id: 'a',
        rule: 'rule_set=chinasite => route(direct)',
        chains: ['direct'],
        start: '2020-01-01T00:00:00Z',
        metadata: {
          host: 'www.bilibili.com',
          processPath: '/usr/lib/firefox/firefox (alice)',
          network: 'tcp',
          destinationPort: '443',
        },
      }),
    ])

    const row = screen.getByLabelText('www.bilibili.com 链接')
    const subtitle = row.querySelector('.conn-row-sub')!
    expect(subtitle).toHaveTextContent('中国站点规则集 → 直连')
    expect(subtitle).toHaveTextContent('firefox')
    expect(subtitle).toHaveTextContent('TCP/443')
  })

  it('splits speed and cumulative traffic into proxy and direct cards with favicons', () => {
    renderModal([
      connection({ id: 'a', chains: ['proxy'], downloadSpeed: 2048, uploadSpeed: 1024, download: 2048, upload: 1024 }),
      connection({ id: 'b', chains: ['direct'], downloadSpeed: 512, download: 4096, metadata: { host: 'www.bilibili.com' } }),
    ])

    const stats = document.querySelector('.path-stats') as HTMLElement
    const [proxyCard, directCard] = stats.querySelectorAll('.path-card')

    // 代理卡：速率与累计只含代理连接；计数 1；favicon 条含 github 图标
    expect(proxyCard).toHaveTextContent('代理')
    expect(proxyCard).toHaveTextContent('2.0 KB/s')
    expect(proxyCard).toHaveTextContent('1.0 KB/s')
    expect(proxyCard).toHaveTextContent('2.0 KB')
    expect(proxyCard).toHaveTextContent('1.0 KB')
    expect(proxyCard).toHaveTextContent('1 条链接')
    expect(proxyCard.querySelector('.path-icons [data-site="github"]')).not.toBeNull()

    // 直连卡：512 B/s 与 4.0 KB，计数 1；favicon 条含 bilibili 图标
    expect(directCard).toHaveTextContent('直连')
    expect(directCard).toHaveTextContent('512 B/s')
    expect(directCard).toHaveTextContent('4.0 KB')
    expect(directCard).toHaveTextContent('1 条链接')
    expect(directCard.querySelector('.path-icons [data-site="bilibili"]')).not.toBeNull()
  })

  it('shows a friendly label for the final fallback rule', () => {
    renderModal([connection({ rule: 'final' })])

    expect(screen.getByText(/兜底规则/)).toBeInTheDocument()
  })

  it('uses a curated mark for known sites and a letter for unknown hosts', () => {
    renderModal([
      connection({ id: 'github' }),
      connection({ id: 'unknown', metadata: { host: 'obscure-lab.internal' } }),
    ])

    expect(document.querySelector('[data-site="github"]')).not.toBeNull()
    expect(document.querySelector('[data-site="letter"]')).toHaveTextContent('O')
  })

  it('filters connections by proxy or direct path', async () => {
    const user = userEvent.setup()
    renderModal([
      connection({ id: 'a', chains: ['proxy'] }),
      connection({
        id: 'c',
        rule: 'rule_set=chinasite => route(direct)',
        chains: ['direct'],
        metadata: { host: 'www.bilibili.com' },
      }),
    ])

    expect(screen.getByRole('button', { name: /全部\s*2/ })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /直连/ }))
    expect(screen.getByText('www.bilibili.com')).toBeInTheDocument()
    expect(screen.queryByText('api.github.com')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /代理/ }))
    expect(screen.getByText('api.github.com')).toBeInTheDocument()
    expect(screen.queryByText('www.bilibili.com')).not.toBeInTheDocument()
  })

  it('sorts rows by combined speed so active links float to top', () => {
    renderModal([
      connection({ id: 'a', downloadSpeed: 10, uploadSpeed: 0, metadata: { host: 'slow.dev' } }),
      connection({ id: 'b', downloadSpeed: 0, uploadSpeed: 500, metadata: { host: 'fast.dev' } }),
    ])

    const rows = screen.getAllByLabelText(/\.dev 链接$/)
    expect(rows[0]).toHaveTextContent('fast.dev')
    expect(rows[1]).toHaveTextContent('slow.dev')
  })

  it('dims idle rows and keeps active rows fully visible', () => {
    renderModal([
      connection({ id: 'a', downloadSpeed: 100, metadata: { host: 'active.dev' } }),
      connection({ id: 'b', metadata: { host: 'idle.dev' } }),
    ])

    expect(screen.getByLabelText('active.dev 链接').className).toContain('active')
    expect(screen.getByLabelText('idle.dev 链接').className).toContain('idle')
  })

  it('tags rows with connection ids as flip keys for reorder animation', () => {
    renderModal([
      connection({ id: 'slow-one', downloadSpeed: 10, metadata: { host: 'slow.dev' } }),
      connection({ id: 'fast-one', downloadSpeed: 500, metadata: { host: 'fast.dev' } }),
    ])

    const rows = screen.getAllByLabelText(/\.dev 链接$/)
    // 速率降序：fast-one 在前；flip key 与连接 id 一致，跨轮询稳定
    expect(rows.map((row) => (row as HTMLElement).dataset.flipKey)).toEqual(['fast-one', 'slow-one'])
  })

  it('bounds DOM rows for large lists and renders the end when scrolled', () => {
    const connections = Array.from({ length: 5000 }, (_, i) => connection({
      id: `item-${i}`, downloadSpeed: 5000 - i, metadata: { host: `host-${i}.dev` },
    }))
    renderModal(connections)
    expect(document.querySelectorAll('.conn-row').length).toBeLessThan(30)
    expect(screen.getByLabelText('host-0.dev 链接')).toBeInTheDocument()
    const list = screen.getByRole('list', { name: '连接列表' })
    fireEvent.scroll(list, { target: { scrollTop: 5000 * 66 } })
    expect(document.querySelectorAll('.conn-row').length).toBeLessThan(30)
    expect(screen.getByLabelText('host-4999.dev 链接')).toBeInTheDocument()
    expect(screen.queryByLabelText('host-0.dev 链接')).not.toBeInTheDocument()
  })

})
