import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
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
      onClose={vi.fn()}
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
    expect(document.querySelector('.connections-live-badge')).toHaveTextContent('3 条链接')
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

  it('splits speed and cumulative traffic into proxy and direct lanes', () => {
    renderModal([
      connection({ id: 'a', chains: ['proxy'], downloadSpeed: 2048, uploadSpeed: 1024, download: 2048, upload: 1024 }),
      connection({ id: 'b', chains: ['direct'], downloadSpeed: 512, download: 4096, metadata: { host: 'www.bilibili.com' } }),
    ])

    const stats = document.querySelector('.path-stats') as HTMLElement
    const lanes = stats.querySelectorAll('.path-lane')
    const [proxyLane, directLane] = lanes

    // 代理通道：速率与累计只含代理连接；计数 1
    expect(proxyLane).toHaveTextContent('代理')
    expect(proxyLane).toHaveTextContent('2.0 KB/s')
    expect(proxyLane).toHaveTextContent('1.0 KB/s')
    expect(proxyLane).toHaveTextContent('2.0 KB')
    expect(proxyLane).toHaveTextContent('1.0 KB')
    expect(proxyLane).toHaveTextContent('1 条链接')

    // 直连通道：512 B/s 与 4.0 KB，计数 1
    expect(directLane).toHaveTextContent('直连')
    expect(directLane).toHaveTextContent('512 B/s')
    expect(directLane).toHaveTextContent('4.0 KB')
    expect(directLane).toHaveTextContent('1 条链接')

    // 占比条：代理 (2048+1024)/3584 ≈ 86%，直连 512/3584 ≈ 14%
    const proxyBar = proxyLane.querySelector('.path-share > i') as HTMLElement
    const directBar = directLane.querySelector('.path-share > i') as HTMLElement
    expect(proxyBar.style.width).toBe('86%')
    expect(directBar.style.width).toBe('14%')
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

  it('renders the speed bar as full-width download/upload segments matching the numbers', () => {
    renderModal([
      connection({ id: 'a', downloadSpeed: 300, uploadSpeed: 100 }),
      connection({ id: 'b', downloadSpeed: 50, metadata: { host: 'only-down.dev' } }),
    ])

    // 每行恒满宽：段宽 = 本行内上下行占比，不随全场最大值缩放
    const rowA = screen.getByLabelText('api.github.com 链接')
    expect((rowA.querySelector('.seg-down') as HTMLElement).style.width).toBe('75%')
    expect((rowA.querySelector('.seg-up') as HTMLElement).style.width).toBe('25%')
    const rowB = screen.getByLabelText('only-down.dev 链接')
    expect((rowB.querySelector('.seg-down') as HTMLElement).style.width).toBe('100%')
    expect((rowB.querySelector('.seg-up') as HTMLElement).style.width).toBe('0%')
  })
})
