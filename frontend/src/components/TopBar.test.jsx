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
    primaryGroup: { now: 'node-a', all: ['node-a'] },
    delays: { 'node-a': 98 },
    testingNodes: {},
    onTestDelay: vi.fn(),
    ...overrides,
  }
  return { ...render(<TopBar {...props} />), props }
}

describe('TopBar merged layout', () => {
  it('融合了品牌 logo、速率、模式与版本号；品牌文字与状态块已移除', () => {
    const { container } = renderTopBar()

    // 品牌：仅 64px logo，无品牌文字、无状态块
    const logo = container.querySelector('.brand-icon')
    expect(logo).toBeInTheDocument()
    expect(logo).toHaveAttribute('width', '64')
    expect(screen.queryByText('Miao')).not.toBeInTheDocument()
    expect(screen.queryByText(/Sing-box/)).not.toBeInTheDocument()
    expect(container.querySelector('.status-left-wrap')).not.toBeInTheDocument()
    expect(container.querySelector('.run-badge')).not.toBeInTheDocument()
    // 速率、模式、版本号都在
    expect(screen.getByTitle('查看链接统计')).toBeInTheDocument()
    expect(screen.getByRole('group', { name: '代理模式' })).toBeInTheDocument()
    expect(screen.getByText('v0.31.0')).toBeInTheDocument()
  })

  it('shows the current node chip and tests its delay on click', async () => {
    const user = userEvent.setup()
    const onTestDelay = vi.fn()
    renderTopBar({ onTestDelay })

    const chip = screen.getByRole('button', { name: '测试当前节点 node-a 延迟' })
    expect(chip).toBeInTheDocument()
    expect(screen.getByText('node-a')).toBeInTheDocument()
    expect(screen.getByText('98 ms')).toBeInTheDocument()

    await user.click(chip)
    expect(onTestDelay).toHaveBeenCalledWith('node-a')
  })

  it('disables the current node chip when there is no current node', () => {
    renderTopBar({ primaryGroup: null })

    const chip = screen.getByRole('button', { name: '当前节点' })
    expect(chip).toBeDisabled()
    expect(screen.getByText('未选择')).toBeInTheDocument()
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
