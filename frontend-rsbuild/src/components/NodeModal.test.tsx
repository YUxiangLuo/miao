import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { NodeModal } from './NodeModal'
import { EMPTY_NODE_FORM } from '../utils'

function renderModal(props = {}) {
  return render(
    <NodeModal
      open
      nodeType="hysteria2"
      setNodeType={rs.fn()}
      form={{ ...EMPTY_NODE_FORM }}
      setForm={rs.fn()}
      loading={false}
      onClose={rs.fn()}
      onSubmit={rs.fn()}
      onImport={rs.fn()}
      onDeployVps={rs.fn()}
      {...props}
    />,
  )
}

describe('NodeModal link import', () => {
  it('parses pasted links into preview cards and imports them', async () => {
    const user = userEvent.setup()
    const onImport = rs.fn()
    renderModal({ onImport })

    const input = screen.getByRole('textbox', { name: '节点分享链接' })
    await user.click(input)
    await user.paste('hysteria2://password123@example.com:443?sni=mask.com#我的节点')

    expect(screen.getByText('我的节点')).toBeInTheDocument()
    expect(screen.getByText('Hysteria2')).toBeInTheDocument()
    expect(screen.getByText('example.com:443')).toBeInTheDocument()
    expect(screen.getByText('SNI: mask.com')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '添加 1 个节点' }))
    expect(onImport).toHaveBeenCalledTimes(1)
    const [payloads] = onImport.mock.calls[0] as [Array<{ node_type?: string }>]
    expect(payloads).toHaveLength(1)
    expect(payloads[0]).toMatchObject({
      node_type: 'hysteria2',
      tag: '我的节点',
      server: 'example.com',
      server_port: 443,
      password: 'password123',
      sni: 'mask.com',
    })
  })

  it('shows per-line errors and only imports valid links', async () => {
    const user = userEvent.setup()
    const onImport = rs.fn()
    renderModal({ onImport })

    const input = screen.getByRole('textbox', { name: '节点分享链接' })
    await user.click(input)
    await user.paste('trojan://password123@tj.example.com:443#好节点\nnot-a-link\nanytls://password123@any.example.com:443')

    expect(screen.getByText('不支持的链接类型: not-a-link')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '添加 2 个节点' })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '添加 2 个节点' }))
    const [payloads] = onImport.mock.calls[0] as [Array<{ node_type?: string }>]
    expect(payloads.map((p) => p.node_type)).toEqual(['trojan', 'anytls'])
  })

  it('keeps the submit button disabled until a valid link is pasted', async () => {
    const user = userEvent.setup()
    renderModal()

    const button = screen.getByRole('button', { name: '添加节点' })
    expect(button).toBeDisabled()

    const input = screen.getByRole('textbox', { name: '节点分享链接' })
    await user.click(input)
    await user.paste('hysteria2://password123@example.com:443#x')
    expect(screen.getByRole('button', { name: '添加 1 个节点' })).toBeEnabled()
  })

  it('switches to the manual form', async () => {
    const user = userEvent.setup()
    renderModal()

    await user.click(screen.getByRole('button', { name: '手动填写' }))
    expect(screen.getByLabelText('协议')).toBeInTheDocument()
    expect(screen.getByLabelText('节点名称')).toBeInTheDocument()
    // 高级选项默认折叠(SNI 等字段在 details 内但不可见)
    const details = document.querySelector('.advanced-details')
    expect(details).not.toHaveAttribute('open')
    expect(screen.getByLabelText(/SNI/)).toBeInTheDocument()
  })

  it('deploys a vps with ip and root password', async () => {
    const user = userEvent.setup()
    const onDeployVps = rs.fn().mockResolvedValue(true)
    renderModal({ onDeployVps })

    await user.click(screen.getByRole('button', { name: 'VPS 部署' }))
    const passwordInput = screen.getByLabelText('root 密码')
    expect(passwordInput).toHaveAttribute('type', 'password')

    await user.type(screen.getByLabelText('VPS IP 地址'), '203.0.113.10')
    await user.type(passwordInput, 'rootpass')
    await user.click(screen.getByRole('button', { name: '开始部署' }))

    expect(onDeployVps).toHaveBeenCalledWith({ ip: '203.0.113.10', password: 'rootpass' })
  })

  it('hides vps deploy when the platform does not support askpass', () => {
    renderModal({ vpsSupported: false })

    expect(screen.queryByRole('button', { name: 'VPS 部署' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '粘贴链接' })).toBeInTheDocument()
  })
})
