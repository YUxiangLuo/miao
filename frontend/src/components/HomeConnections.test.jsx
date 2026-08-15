import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { HomeConnections } from './HomeConnections.jsx'

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
      ...metadata,
    },
  }
}

describe('HomeConnections', () => {
  it('renders an empty placeholder when the service is stopped', () => {
    render(
      <HomeConnections
        status={{ running: false }}
        data={{
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
        status={{ running: true }}
        data={{
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
        status={{ running: true }}
        data={{
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
  })

  it('opens the full connections view from the strip', async () => {
    const user = userEvent.setup()
    const onOpenAll = vi.fn()
    render(
      <HomeConnections
        status={{ running: true }}
        data={{ connections: [connection({ id: 'a', downloadSpeed: 80 })] }}
        onOpenAll={onOpenAll}
      />,
    )

    await user.click(screen.getByRole('button', { name: '查看全部' }))
    expect(onOpenAll).toHaveBeenCalledTimes(1)
  })
})
