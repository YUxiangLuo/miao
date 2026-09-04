import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { OnboardingScreen } from './OnboardingScreen'
import type { VergeImportItem } from '../types/api'

function renderOnboarding(props = {}) {
  return render(
    <OnboardingScreen
      onAddSub={rs.fn()}
      pendingActions={new Set()}
      onOpenAddNode={rs.fn()}
      showToast={rs.fn()}
      onScanVerge={rs.fn().mockResolvedValue(null)}
      onImportVerge={rs.fn().mockResolvedValue(true)}
      {...props}
    />
  )
}

function vergeItem(overrides: Partial<VergeImportItem> = {}): VergeImportItem {
  return {
    name: '香港机场',
    url: 'https://example.com/sub?token=abc',
    already_added: false,
    ...overrides,
  }
}

describe('OnboardingScreen', () => {
  it('submits a trimmed subscription URL', async () => {
    const user = userEvent.setup()
    const onAddSub = rs.fn()

    renderOnboarding({ onAddSub })

    await user.type(screen.getByPlaceholderText('粘贴订阅链接...'), '  https://example.com/sub  ')
    await user.click(screen.getByRole('button', { name: /添加订阅/ }))

    expect(onAddSub).toHaveBeenCalledWith('https://example.com/sub')
  })

  it('shows validation errors instead of submitting invalid URLs', async () => {
    const user = userEvent.setup()
    const onAddSub = rs.fn()
    const showToast = rs.fn()

    renderOnboarding({ onAddSub, showToast })

    await user.type(screen.getByPlaceholderText('粘贴订阅链接...'), 'not-a-url')
    await user.click(screen.getByRole('button', { name: /添加订阅/ }))

    expect(onAddSub).not.toHaveBeenCalled()
    expect(showToast).toHaveBeenCalledWith('无效的订阅链接格式', 'error')
  })

  it('opens the manual node modal', async () => {
    const user = userEvent.setup()
    const onOpenAddNode = rs.fn()

    renderOnboarding({ onOpenAddNode })

    await user.click(screen.getByRole('button', { name: /手动添加节点/ }))

    expect(onOpenAddNode).toHaveBeenCalledTimes(1)
  })

  it('informs when no Clash Verge subscriptions are detected', async () => {
    const user = userEvent.setup()
    const onScanVerge = rs.fn().mockResolvedValue({ found: false, items: [] })
    const showToast = rs.fn()

    renderOnboarding({ onScanVerge, showToast })

    await user.click(screen.getByRole('button', { name: /从 Clash Verge Rev 导入/ }))

    expect(onScanVerge).toHaveBeenCalledTimes(1)
    expect(showToast).toHaveBeenCalledWith('未检测到 Clash Verge Rev 的订阅', 'info')
    expect(screen.queryByText(/检测到 Clash Verge Rev/)).not.toBeInTheDocument()
  })

  it('informs when everything detected is already added', async () => {
    const user = userEvent.setup()
    const onScanVerge = rs.fn().mockResolvedValue({
      found: true,
      items: [vergeItem({ already_added: true })],
    })
    const showToast = rs.fn()

    renderOnboarding({ onScanVerge, showToast })

    await user.click(screen.getByRole('button', { name: /从 Clash Verge Rev 导入/ }))

    expect(showToast).toHaveBeenCalledWith('检测到的订阅都已在列表中', 'info')
    expect(screen.queryByText(/检测到 Clash Verge Rev 的 \d+ 条订阅/)).not.toBeInTheDocument()
  })

  it('lists detected subscriptions with new ones preselected and added ones disabled', async () => {
    const user = userEvent.setup()
    const onScanVerge = rs.fn().mockResolvedValue({
      found: true,
      items: [
        vergeItem(),
        vergeItem({ name: '备用', url: 'https://backup.example/feed', already_added: true }),
      ],
    })

    renderOnboarding({ onScanVerge })

    await user.click(screen.getByRole('button', { name: /从 Clash Verge Rev 导入/ }))

    expect(screen.getByText('检测到 Clash Verge Rev 的 2 条订阅')).toBeInTheDocument()
    const newItem = screen.getByRole('checkbox', { name: /香港机场/ })
    const addedItem = screen.getByRole('checkbox', { name: /备用/ })
    expect(newItem).toBeChecked()
    expect(newItem).toBeEnabled()
    expect(addedItem).toBeChecked()
    expect(addedItem).toBeDisabled()
    expect(screen.getByText('已添加')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /导入 1 条/ })).toBeEnabled()
  })

  it('imports only the selected subscriptions', async () => {
    const user = userEvent.setup()
    const items = [
      vergeItem(),
      vergeItem({ name: '日本', url: 'https://jp.example/sub' }),
    ]
    const onScanVerge = rs.fn().mockResolvedValue({ found: true, items })
    const onImportVerge = rs.fn().mockResolvedValue(true)

    renderOnboarding({ onScanVerge, onImportVerge })

    await user.click(screen.getByRole('button', { name: /从 Clash Verge Rev 导入/ }))
    // 取消勾选「日本」，只导一条
    await user.click(screen.getByRole('checkbox', { name: /日本/ }))
    await user.click(screen.getByRole('button', { name: /导入 1 条/ }))

    expect(onImportVerge).toHaveBeenCalledWith(['https://example.com/sub?token=abc'])
  })

  it('collapses the panel on cancel without importing', async () => {
    const user = userEvent.setup()
    const onScanVerge = rs.fn().mockResolvedValue({ found: true, items: [vergeItem()] })
    const onImportVerge = rs.fn()

    renderOnboarding({ onScanVerge, onImportVerge })

    await user.click(screen.getByRole('button', { name: /从 Clash Verge Rev 导入/ }))
    await user.click(screen.getByRole('button', { name: '取消' }))

    expect(onImportVerge).not.toHaveBeenCalled()
    expect(screen.queryByText(/检测到 Clash Verge Rev 的 \d+ 条订阅/)).not.toBeInTheDocument()
  })
})
