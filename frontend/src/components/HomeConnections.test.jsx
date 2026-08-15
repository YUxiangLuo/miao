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
  it('hides when the service is stopped', () => {
    const { container } = render(
      <HomeConnections
        status={{ running: false }}
        data={{
          connections: [connection({ id: 'a', downloadSpeed: 1200 })],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    expect(container).toBeEmptyDOMElement()
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
