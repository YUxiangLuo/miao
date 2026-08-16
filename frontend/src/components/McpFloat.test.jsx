import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { McpFloat } from './McpFloat.jsx'

function renderFloat(props = {}) {
  return render(
    <McpFloat
      enabled={false}
      pending={false}
      onToggle={vi.fn()}
      showToast={vi.fn()}
      {...props}
    />,
  )
}

describe('McpFloat', () => {
  it('reflects the toggle state and calls onToggle with the target state', async () => {
    const user = userEvent.setup()
    const onToggle = vi.fn()
    renderFloat({ enabled: false, onToggle })

    const toggle = screen.getByRole('switch', { name: 'MCP 端点开关' })
    expect(toggle).toHaveAttribute('aria-checked', 'false')

    await user.click(toggle)
    expect(onToggle).toHaveBeenCalledWith(true)
  })

  it('shows enabled state and blocks clicks while pending', () => {
    renderFloat({ enabled: true, pending: true })

    const toggle = screen.getByRole('switch', { name: 'MCP 端点开关' })
    expect(toggle).toHaveAttribute('aria-checked', 'true')
    expect(toggle).toBeDisabled()
  })

  it('copies the MCP address and shows a toast', async () => {
    const user = userEvent.setup()
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    })
    const showToast = vi.fn()
    renderFloat({ showToast })

    await user.click(screen.getByRole('button', { name: '复制 MCP 地址' }))

    expect(writeText).toHaveBeenCalledWith(`${window.location.origin}/mcp`)
    expect(showToast).toHaveBeenCalledWith(
      expect.stringContaining('/mcp'),
      'success',
    )
  })
})
