import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { TopBar, type TopBarProps } from './TopBar'
import { statusMock } from '../testFixtures'

function renderTopBar(overrides: Partial<TopBarProps> = {}) {
  const props: TopBarProps = {
    status: statusMock({ running: true }),
    traffic: { up: 0, down: 0 },
    versionInfo: {
      current: 'v0.31.0',
      latest: null,
      has_update: false,
      download_url: null,
      upgrade_supported: true,
    },
    upgrading: false,
    onUpgradeClick: rs.fn(),
    pendingActions: new Set<string>(),
    onSetRouteMode: rs.fn(),
    onOpenConnections: rs.fn(),
    primaryGroup: { type: 'Selector', now: 'node-a', all: ['node-a'] },
    delays: { 'node-a': 98 },
    testingNodes: {},
    onTestDelay: rs.fn(),
    mcpEnabled: true,
    mcpPending: false,
    onToggleMcp: rs.fn(),
    showToast: rs.fn(),
    ...overrides,
  }
  return { ...render(<TopBar {...props} />), props }
}

describe('TopBar merged layout', () => {
  it('keeps network history accessible when the kernel is stopped', async () => {
    const user = userEvent.setup()
    const { props } = renderTopBar({ status: statusMock({ ready: false, running: false }) })
    await user.click(screen.getByRole('button', { name: '查看网络活动分析与链接统计' }))
    expect(props.onOpenConnections).toHaveBeenCalledOnce()
  })

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
    expect(screen.getByLabelText('MCP 控制')).toBeInTheDocument()
    expect(screen.getByRole('group', { name: '代理模式' })).toBeInTheDocument()
    expect(screen.getByText('v0.31.0')).toBeInTheDocument()
  })

  it('shows the current node chip and tests its delay on click', async () => {
    const user = userEvent.setup()
    const onTestDelay = rs.fn()
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

  it.each([{ ready: false }, { ready: true, initializing: true }])('disables current node testing until runtime is ready: %j', async (state) => {
    const user = userEvent.setup()
    const { props } = renderTopBar({ status: statusMock({ running: true, ...state }) })
    const chip = screen.getByRole('button', { name: '测试当前节点 node-a 延迟' })
    expect(chip).toBeDisabled()
    expect(chip).toHaveAttribute('title', '代理就绪后可测试延迟')
    await user.click(chip)
    expect(props.onTestDelay).not.toHaveBeenCalled()
  })

  it('does not offer in-app upgrade when the platform cannot replace the binary', () => {
    const onUpgradeClick = rs.fn()
    const { container } = renderTopBar({
      versionInfo: {
        current: 'v0.31.0',
        latest: 'v0.32.0',
        has_update: true,
        download_url: null,
        upgrade_supported: false,
      },
      onUpgradeClick,
    })

    // 不提供面板内升级按钮，但仍提示有新版本
    expect(screen.queryByRole('button', { name: /v0/ })).not.toBeInTheDocument()
    expect(screen.getByText('v0.31.0')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /下载 v0.32.0/ })).toHaveAttribute('href', 'https://github.com/YUxiangLuo/miao/releases/latest')
    expect(container.querySelector('.version-dot')).toBeInTheDocument()
    expect(container.querySelector('.version-chip')).toHaveClass('has-update')
    expect(onUpgradeClick).not.toHaveBeenCalled()
  })

  it('keeps the upgrade button when self-update is supported', async () => {
    const user = userEvent.setup()
    const onUpgradeClick = rs.fn()
    renderTopBar({
      versionInfo: {
        current: 'v0.31.0',
        latest: 'v0.32.0',
        has_update: true,
        download_url: null,
        upgrade_supported: true,
      },
      onUpgradeClick,
    })

    expect(screen.getByText('v0.31.0')).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: /可更新至 v0.32.0/ }))
    expect(onUpgradeClick).toHaveBeenCalledTimes(1)
  })
})
