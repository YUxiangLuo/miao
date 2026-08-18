import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { TopBar } from './TopBar.jsx'

const running = { running: true, initializing: false, route_mode: 'rule' }

function renderTopBar(overrides = {}) {
  const props = {
    status: running,
    traffic: { up: 0, down: 0 },
    versionInfo: { current: 'v0.31.0', has_update: false },
    upgrading: false,
    onUpgradeClick: vi.fn(),
    loadingAction: null,
    onSetRouteMode: vi.fn(),
    onOpenConnections: vi.fn(),
    ...overrides,
  }
  return { ...render(<TopBar {...props} />), props }
}

describe('TopBar merged layout', () => {
  it('融合了品牌、状态、速率、模式与版本号，且无独立的运行中徽章', () => {
    const { container } = renderTopBar()

    // 品牌
    expect(screen.getByText('Miao')).toBeInTheDocument()
    expect(container.querySelector('.brand-icon')).toBeInTheDocument()
    // 状态文案只出现一次（原 header 的 run-badge 已删除）
    expect(screen.getByText(/Sing-box 运行中/)).toBeInTheDocument()
    expect(container.querySelector('.run-badge')).not.toBeInTheDocument()
    // 速率、模式、版本号都在
    expect(screen.getByTitle('查看链接统计')).toBeInTheDocument()
    expect(screen.getByRole('group', { name: '代理模式' })).toBeInTheDocument()
    expect(screen.getByText('v0.31.0')).toBeInTheDocument()
  })

  it('does not offer in-app upgrade when the platform cannot replace the binary', () => {
    const onUpgradeClick = vi.fn()
    const { container } = renderTopBar({
      versionInfo: {
        current: 'v0.31.0',
        latest: 'v0.32.0',
        has_update: true,
        upgrade_supported: false,
      },
      onUpgradeClick,
    })

    // 不提供面板内升级按钮，但仍提示有新版本
    expect(screen.queryByRole('button', { name: /v0/ })).not.toBeInTheDocument()
    expect(screen.getByText('v0.32.0')).toBeInTheDocument()
    expect(container.querySelector('.version-dot')).toBeInTheDocument()
    expect(container.querySelector('.version-chip')).toHaveClass('has-update')
    expect(onUpgradeClick).not.toHaveBeenCalled()
  })

  it('keeps the upgrade button when self-update is supported', async () => {
    const user = userEvent.setup()
    const onUpgradeClick = vi.fn()
    renderTopBar({
      versionInfo: {
        current: 'v0.31.0',
        latest: 'v0.32.0',
        has_update: true,
        upgrade_supported: true,
      },
      onUpgradeClick,
    })

    await user.click(screen.getByRole('button', { name: /v0.32.0/ }))
    expect(onUpgradeClick).toHaveBeenCalledTimes(1)
  })
})
