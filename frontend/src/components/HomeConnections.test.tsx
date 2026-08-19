import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { HomeConnections } from './HomeConnections'
import { connectionMock, statusMock } from '../testFixtures'
import type { EnrichedConnection } from '../types/clash'

function connection({ metadata, ...overrides }: Partial<EnrichedConnection> = {}) {
  return connectionMock({
    ...overrides,
    metadata: {
      host: 'api.github.com',
      destinationPort: '443',
      network: 'tcp',
      ...metadata,
    },
  })
}

describe('HomeConnections', () => {
  it('renders an empty placeholder when the service is stopped', () => {
    render(
      <HomeConnections
        status={statusMock({ running: false })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [connection({ id: 'a', downloadSpeed: 1200 })],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    // 布局占位始终存在,避免活跃链接出现/消失时主内容区高度跳动
    expect(screen.getByRole('region', { name: '活跃链接' })).toBeInTheDocument()
    expect(screen.getByText('暂无活跃链接')).toBeInTheDocument()
    expect(screen.queryByText('api.github.com')).not.toBeInTheDocument()
  })

  it('renders an empty placeholder when all connections are idle', () => {
    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [connection({ id: 'idle', downloadSpeed: 0, uploadSpeed: 0 })],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    expect(screen.getByText('暂无活跃链接')).toBeInTheDocument()
    expect(screen.getByText('0')).toBeInTheDocument()
  })

  it('hides idle connections and lists active site cards by speed', () => {
    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [
            connection({ id: 'idle', downloadSpeed: 0, metadata: { host: 'idle.dev' } }),
            connection({ id: 'slow', downloadSpeed: 10, metadata: { host: 'slow.dev' } }),
            connection({ id: 'fast', downloadSpeed: 900, metadata: { host: 'fast.dev' } }),
          ],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    expect(screen.getByRole('region', { name: '活跃链接' })).toBeInTheDocument()
    expect(screen.queryByText('idle.dev')).not.toBeInTheDocument()
    const domains = screen.getAllByText(/\.dev$/).map((node) => node.textContent)
    expect(domains).toEqual(['fast.dev', 'slow.dev'])
    expect(screen.getByText('2')).toBeInTheDocument()
    // 出口 chip：直连显示 direct，代理显示链路第一节
    expect(screen.getAllByText('proxy').length).toBeGreaterThan(0)
  })

  it('opens the full connections view from the strip', async () => {
    const user = userEvent.setup()
    const onOpenAll = vi.fn()
    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{ connections: [connection({ id: 'a', downloadSpeed: 80 })] }}
        onOpenAll={onOpenAll}
      />,
    )

    await user.click(screen.getByRole('button', { name: '查看全部' }))
    expect(onOpenAll).toHaveBeenCalledTimes(1)
  })

  it('renders strip cards as static (details live in the full view)', () => {
    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{ connections: [connection({ id: 'a', downloadSpeed: 80 })] }}
        onOpenAll={vi.fn()}
      />,
    )

    // 条带单行裁切,卡片不提供展开入口;明细由「查看全部」承载。
    // 卡片只显示主域名,完整域名在 title 里
    expect(screen.queryByText('api.github.com')).not.toBeInTheDocument()
    expect(screen.getByText('github.com')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /链接详情/ })).not.toBeInTheDocument()
  })

  it('takes the letter fallback from the main domain, not the subdomain', () => {
    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{ connections: [connection({ id: 'a', downloadSpeed: 80, metadata: { host: 'api.kimi.com' } })] }}
        onOpenAll={vi.fn()}
      />,
    )

    // kimi.com 无品牌图标,字母块应取主域名首字母 K 而非子域名的 A
    expect(screen.getByText('kimi.com')).toBeInTheDocument()
    expect(screen.getByText('K')).toBeInTheDocument()
    expect(screen.queryByText('A')).not.toBeInTheDocument()
  })
})
