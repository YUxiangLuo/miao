import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { OnboardingScreen } from './OnboardingScreen.jsx'

function renderOnboarding(props = {}) {
  return render(
    <OnboardingScreen
      onAddSub={vi.fn()}
      loadingAction=""
      onOpenAddNode={vi.fn()}
      showToast={vi.fn()}
      {...props}
    />
  )
}

describe('OnboardingScreen', () => {
  it('submits a trimmed subscription URL', async () => {
    const user = userEvent.setup()
    const onAddSub = vi.fn()

    renderOnboarding({ onAddSub })

    await user.type(screen.getByPlaceholderText('粘贴订阅链接...'), '  https://example.com/sub  ')
    await user.click(screen.getByRole('button', { name: /添加订阅/ }))

    expect(onAddSub).toHaveBeenCalledWith('https://example.com/sub')
  })

  it('shows validation errors instead of submitting invalid URLs', async () => {
    const user = userEvent.setup()
    const onAddSub = vi.fn()
    const showToast = vi.fn()

    renderOnboarding({ onAddSub, showToast })

    await user.type(screen.getByPlaceholderText('粘贴订阅链接...'), 'not-a-url')
    await user.click(screen.getByRole('button', { name: /添加订阅/ }))

    expect(onAddSub).not.toHaveBeenCalled()
    expect(showToast).toHaveBeenCalledWith('无效的订阅链接格式', 'error')
  })

  it('opens the manual node modal', async () => {
    const user = userEvent.setup()
    const onOpenAddNode = vi.fn()

    renderOnboarding({ onOpenAddNode })

    await user.click(screen.getByRole('button', { name: /手动添加节点/ }))

    expect(onOpenAddNode).toHaveBeenCalledTimes(1)
  })
})
