import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { McpControl } from './McpControl'

function renderControl(props = {}) {
  return render(
    <McpControl
      enabled={false}
      pending={false}
      onToggle={rs.fn()}
      showToast={rs.fn()}
      {...props}
    />,
  )
}

describe('McpControl', () => {
  it('reflects the toggle state and calls onToggle with the target state', async () => {
    const user = userEvent.setup()
    const onToggle = rs.fn()
    renderControl({ enabled: false, onToggle })

    const toggle = screen.getByRole('switch', { name: 'MCP 端点开关' })
    expect(toggle).toHaveAttribute('aria-checked', 'false')

    await user.click(toggle)
    expect(onToggle).toHaveBeenCalledWith(true)
  })

  it('shows enabled state and blocks clicks while pending', () => {
    renderControl({ enabled: true, pending: true })

    const toggle = screen.getByRole('switch', { name: 'MCP 端点开关' })
    expect(toggle).toHaveAttribute('aria-checked', 'true')
    expect(toggle).toBeDisabled()
  })

  it('copies the exact MCP add command for each agent', async () => {
    const user = userEvent.setup()
    const writeText = rs.fn().mockResolvedValue(undefined)
    const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    })
    try {
      const showToast = rs.fn()
      renderControl({ showToast })
      const url = `${window.location.origin}/mcp`

      await user.click(screen.getByRole('button', { name: '复制 Claude MCP 添加命令' }))
      await user.click(screen.getByRole('button', { name: '复制 Codex MCP 添加命令' }))
      expect(screen.getAllByRole('button')).toHaveLength(3)

      expect(writeText).toHaveBeenNthCalledWith(1, `claude mcp add --transport http miao ${url}`)
      expect(writeText).toHaveBeenNthCalledWith(2, `codex mcp add miao --url ${url}`)
      expect(writeText).toHaveBeenCalledTimes(2)
      expect(showToast).toHaveBeenNthCalledWith(1, '已复制 Claude MCP 添加命令', 'success')
      expect(showToast).toHaveBeenNthCalledWith(2, '已复制 Codex MCP 添加命令', 'success')
      await user.click(screen.getByRole('button', { name: '复制 AGENTS.md 说明' }))
      const markdown = writeText.mock.calls[2][0] as string
      expect(markdown).toMatch(/^## Miao MCP\n/)
      expect(markdown).toContain(url)
      expect(markdown).toContain('在面板启用 MCP')
      expect(markdown).toContain('不会自动配置 MCP')
      expect(markdown).toContain('get_status')
      expect(markdown).toContain('confirm: true')
      expect(markdown).toContain('超时不等于失败')
      expect(markdown.length).toBeLessThan(850)
    } finally {
      if (clipboardDescriptor) Object.defineProperty(navigator, 'clipboard', clipboardDescriptor)
      else Reflect.deleteProperty(navigator, 'clipboard')
    }
  })

  it('reports fallback copy failure and removes the temporary field', async () => {
    const user = userEvent.setup()
    const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
    const execCommandDescriptor = Object.getOwnPropertyDescriptor(document, 'execCommand')
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined })
    Object.defineProperty(document, 'execCommand', {
      configurable: true,
      value: rs.fn().mockReturnValue(false),
    })
    try {
      const showToast = rs.fn()
      renderControl({ showToast })

      await user.click(screen.getByRole('button', { name: '复制 Claude MCP 添加命令' }))

      const field = screen.getByRole('textbox', { name: '待复制文本' }) as HTMLTextAreaElement
      expect(field).toHaveValue(`claude mcp add --transport http miao ${window.location.origin}/mcp`)
      expect(field).toHaveFocus()
      expect(field.selectionEnd).toBe(field.value.length)
      expect(document.querySelectorAll('textarea')).toHaveLength(1)
      expect(showToast).not.toHaveBeenCalled()
      await user.keyboard('{Escape}')
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
      expect(screen.getByRole('button', { name: '复制 Claude MCP 添加命令' })).toHaveFocus()
      await user.click(screen.getByRole('button', { name: '复制 AGENTS.md 说明' }))
      expect((screen.getByRole('textbox', { name: '待复制文本' }) as HTMLTextAreaElement).value).toContain('## Miao MCP')
    } finally {
      if (clipboardDescriptor) Object.defineProperty(navigator, 'clipboard', clipboardDescriptor)
      else Reflect.deleteProperty(navigator, 'clipboard')
      if (execCommandDescriptor) Object.defineProperty(document, 'execCommand', execCommandDescriptor)
      else Reflect.deleteProperty(document, 'execCommand')
    }
  })
})
