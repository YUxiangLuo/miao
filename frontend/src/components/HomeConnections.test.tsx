import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { HomeConnections } from './HomeConnections'
import { foldHomeConnections } from './homeConnectionsFold'
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

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('HomeConnections', () => {
  it('reserves room for +N when site cards overflow the measured width', () => {
    expect(foldHomeConnections(3, 452)).toEqual({ shown: 3, more: 0 })
    expect(foldHomeConnections(4, 452)).toEqual({ shown: 2, more: 2 })
    expect(foldHomeConnections(2, 150)).toEqual({ shown: 0, more: 2 })
  })

  it('recalculates hidden links after ResizeObserver reports a width change', async () => {
    const user = userEvent.setup()
    const onOpenAll = vi.fn()
    let clientWidth = 452
    let resizeCallback: ResizeObserverCallback | undefined
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => clientWidth)
    vi.stubGlobal('ResizeObserver', class {
      constructor(callback: ResizeObserverCallback) {
        resizeCallback = callback
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    })

    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          connections: [
            connection({ id: 'a', downloadSpeed: 40, metadata: { host: 'one.dev' } }),
            connection({ id: 'b', downloadSpeed: 30, metadata: { host: 'two.dev' } }),
            connection({ id: 'c', downloadSpeed: 20, metadata: { host: 'three.dev' } }),
            connection({ id: 'd', downloadSpeed: 10, metadata: { host: 'four.dev' } }),
          ],
        }}
        onOpenAll={onOpenAll}
      />,
    )

    expect(screen.getByText('one.dev')).toBeInTheDocument()
    expect(screen.getByText('two.dev')).toBeInTheDocument()
    expect(screen.queryByText('three.dev')).not.toBeInTheDocument()
    const more = screen.getByRole('button', { name: '打开其余 2 个活跃链接' })
    expect(more).toHaveTextContent('+2')
    await user.click(more)
    expect(onOpenAll).toHaveBeenCalledTimes(1)

    clientWidth = 606
    act(() => resizeCallback?.([], {} as ResizeObserver))

    expect(screen.queryByText('+2')).not.toBeInTheDocument()
    expect(screen.getByText('three.dev')).toBeInTheDocument()
    expect(screen.getByText('four.dev')).toBeInTheDocument()
  })

  it('falls back to window resize events without ResizeObserver', () => {
    let clientWidth = 452
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => clientWidth)
    vi.stubGlobal('ResizeObserver', undefined)

    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          connections: [
            connection({ id: 'a', downloadSpeed: 40, metadata: { host: 'one.dev' } }),
            connection({ id: 'b', downloadSpeed: 30, metadata: { host: 'two.dev' } }),
            connection({ id: 'c', downloadSpeed: 20, metadata: { host: 'three.dev' } }),
            connection({ id: 'd', downloadSpeed: 10, metadata: { host: 'four.dev' } }),
          ],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    expect(screen.getByText('+2')).toBeInTheDocument()
    clientWidth = 606
    act(() => window.dispatchEvent(new Event('resize')))
    expect(screen.queryByText('+2')).not.toBeInTheDocument()
    expect(screen.getByText('four.dev')).toBeInTheDocument()
  })

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
    // 出口 chip：直连显示 direct，代理显示链路第一节
    expect(screen.getAllByText('proxy').length).toBeGreaterThan(0)
  })

  it('renders strip cards as static (details live in the full view)', () => {
    render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{ connections: [connection({ id: 'a', downloadSpeed: 80 })] }}
        onOpenAll={vi.fn()}
      />,
    )

    // 卡片不提供展开入口；只有内容溢出时才由 +N 进入明细。
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

  it('staggers card entrance via the --i custom property', () => {
    const { container } = render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [
            connection({ id: 'a', downloadSpeed: 900, metadata: { host: 'fast.dev' } }),
            connection({ id: 'b', downloadSpeed: 10, metadata: { host: 'slow.dev' } }),
          ],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    const cards = container.querySelectorAll<HTMLElement>('.home-site-card')
    expect([...cards].map((card) => card.style.getPropertyValue('--i'))).toEqual(['0', '1'])
  })

  it('prepends newly appeared groups to the front', () => {
    const { rerender } = render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [
            connection({ id: 'a', downloadSpeed: 900, metadata: { host: 'fast.dev' } }),
            connection({ id: 'b', downloadSpeed: 10, metadata: { host: 'slow.dev' } }),
          ],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    // 新组速率再低也排最前（速率降序只决定同一轮首次落位）
    rerender(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [
            connection({ id: 'a', downloadSpeed: 900, metadata: { host: 'fast.dev' } }),
            connection({ id: 'b', downloadSpeed: 10, metadata: { host: 'slow.dev' } }),
            connection({ id: 'c', downloadSpeed: 5, metadata: { host: 'new.dev' } }),
          ],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    const domains = screen.getAllByText(/\.dev$/).map((node) => node.textContent)
    expect(domains).toEqual(['new.dev', 'fast.dev', 'slow.dev'])
  })

  it('keeps card positions stable when speeds change between polls', () => {
    const { rerender } = render(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [
            connection({ id: 'a', downloadSpeed: 900, metadata: { host: 'fast.dev' } }),
            connection({ id: 'b', downloadSpeed: 10, metadata: { host: 'slow.dev' } }),
          ],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    // 速率倒挂也不重排——位置只随「出现/消失」变化
    rerender(
      <HomeConnections
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [
            connection({ id: 'a', downloadSpeed: 10, metadata: { host: 'fast.dev' } }),
            connection({ id: 'b', downloadSpeed: 900, metadata: { host: 'slow.dev' } }),
          ],
        }}
        onOpenAll={vi.fn()}
      />,
    )

    const domains = screen.getAllByText(/\.dev$/).map((node) => node.textContent)
    expect(domains).toEqual(['fast.dev', 'slow.dev'])
  })

  it('treats a vanished-and-returned group as new (front again)', () => {
    const props = {
      status: statusMock({ running: true }),
      onOpenAll: vi.fn(),
    }
    const withBoth = {
      uploadTotal: 0,
      downloadTotal: 0,
      connections: [
        connection({ id: 'a', downloadSpeed: 900, metadata: { host: 'fast.dev' } }),
        connection({ id: 'b', downloadSpeed: 10, metadata: { host: 'slow.dev' } }),
      ],
    }
    const { rerender } = render(<HomeConnections {...props} data={withBoth} />)

    rerender(
      <HomeConnections
        {...props}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [connection({ id: 'a', downloadSpeed: 900, metadata: { host: 'fast.dev' } })],
        }}
      />,
    )
    rerender(<HomeConnections {...props} data={withBoth} />)

    const domains = screen.getAllByText(/\.dev$/).map((node) => node.textContent)
    expect(domains).toEqual(['slow.dev', 'fast.dev'])
  })
})
