import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { VpsErrorDetails } from './VpsErrorDetails'

const command = "sshd -t && sshd -T | grep -E '^(permitrootlogin|passwordauthentication) '"
const text = `解决办法：在目标 VPS 上执行。\n\n\`\`\`sh\n${command}\n\`\`\`\n\n错误详情（退出状态 255）：\nPermission denied (publickey).\n\`\`\`sh\necho untrusted\n\`\`\``

describe('VPS recovery commands', () => {
  it('copies the exact command and leaves raw SSH output as plain text', async () => {
    const user = userEvent.setup()
    const write = rs.spyOn(navigator.clipboard, 'writeText')
    render(<VpsErrorDetails text={text} />)
    expect(screen.getAllByRole('button', { name: '复制命令' })).toHaveLength(1)
    await user.click(screen.getByRole('button', { name: '复制命令' }))
    expect(write).toHaveBeenCalledWith(command)
    expect(screen.getByRole('button', { name: '已复制' })).toBeInTheDocument()
    expect(screen.getByText(/echo untrusted/)).toBeInTheDocument()
    write.mockRestore()
  })

  it('offers manual selection when clipboard permission is denied', async () => {
    const user = userEvent.setup()
    const write = rs.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new Error('denied'))
    render(<VpsErrorDetails text={text} />)
    await user.click(screen.getByRole('button', { name: '复制命令' }))
    expect(screen.getByRole('button', { name: '复制失败，请选中命令手动复制' })).toBeInTheDocument()
    expect(screen.getByText(command)).toBeInTheDocument()
    write.mockRestore()
  })

  it('copies within the dialog on HTTP without Clipboard API and removes the temporary field', async () => {
    const user = userEvent.setup()
    const descriptor = Object.getOwnPropertyDescriptor(navigator, 'clipboard')
    const oldExec = Object.getOwnPropertyDescriptor(document, 'execCommand')
    const exec = rs.fn(() => {
      expect(document.querySelector('.vps-command textarea')).toHaveValue(command)
      return true
    })
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined })
    Object.defineProperty(document, 'execCommand', { configurable: true, value: exec })
    try {
      render(<VpsErrorDetails text={text} />)
      await user.click(screen.getByRole('button', { name: '复制命令' }))
      expect(exec).toHaveBeenCalledWith('copy')
      expect(document.querySelector('.vps-command textarea')).toBeNull()
      expect(screen.getByRole('button', { name: '已复制' })).toHaveFocus()
    } finally {
      if (descriptor) Object.defineProperty(navigator, 'clipboard', descriptor)
      else Reflect.deleteProperty(navigator, 'clipboard')
      if (oldExec) Object.defineProperty(document, 'execCommand', oldExec)
      else Reflect.deleteProperty(document, 'execCommand')
    }
  })
})
