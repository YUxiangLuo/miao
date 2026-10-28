import { useState } from 'react'
import { act, fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { NodeModal } from './NodeModal'
import { EMPTY_NODE_FORM } from '../utils'
import type { BatchNodeResult } from '../types/api'

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

describe('manual node submission', () => {
  it('shows optional SOCKS5 credentials and accepts short protocol-valid passwords', async () => {
    const user = userEvent.setup()
    renderModal({ nodeType: 'socks', form: { ...EMPTY_NODE_FORM, tag: 'SOCKS5', server: 'proxy.example.com', username: 'u', password: 'p' } })
    await user.click(screen.getByRole('button', { name: '手动填写' }))
    expect(screen.getByLabelText('用户名（可选）')).toHaveValue('u')
    expect(screen.getByLabelText('密码（可选）')).toHaveAttribute('type', 'password')
    expect(screen.queryByLabelText('SNI')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '添加 SOCKS5 节点' })).toBeEnabled()
  })

  it.each([0, -1, 1.5, 65536])('blocks invalid port %s with an inline error before submitting', async (port) => {
    const user = userEvent.setup()
    const onSubmit = rs.fn()
    renderModal({ onSubmit, form: { ...EMPTY_NODE_FORM, tag: '测试节点', server: 'example.com', password: 'test-password', server_port: port } })
    await user.click(screen.getByRole('button', { name: '手动填写' }))
    expect(screen.getByRole('alert')).toHaveTextContent(port > 65535 ? '端口号超出范围' : '端口号必须为正整数')
    const submit = screen.getByRole('button', { name: '添加 Hysteria2 节点' })
    expect(submit).toBeDisabled()
    await user.click(submit)
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it.each([1, 65535])('accepts valid port boundary %s', async (port) => {
    const user = userEvent.setup()
    renderModal({ form: { ...EMPTY_NODE_FORM, tag: '测试节点', server: 'example.com', password: 'test-password', server_port: port } })
    await user.click(screen.getByRole('button', { name: '手动填写' }))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '添加 Hysteria2 节点' })).toBeEnabled()
  })

  it('clears the submission validator error when corrected', async () => {
    const user = userEvent.setup()
    function Harness() {
      const [form, setForm] = useState({ ...EMPTY_NODE_FORM, tag: '测试节点', server: 'example.com', password: 'short' })
      return <NodeModal open nodeType="hysteria2" setNodeType={rs.fn()} form={form} setForm={setForm} loading={false}
        onClose={rs.fn()} onSubmit={rs.fn()} onImport={rs.fn()} onDeployVps={rs.fn()} />
    }
    render(<Harness />)
    await user.click(screen.getByRole('button', { name: '手动填写' }))
    expect(screen.getByRole('alert')).toHaveTextContent('密码太短（至少 8 个字符）')
    await user.type(screen.getByLabelText('密码'), '-long-enough')
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: '添加 Hysteria2 节点' })).toBeEnabled()
  })

  it('locks the draft, prevents duplicate clicks, and does not close a reopened dialog', async () => {
    const user = userEvent.setup()
    let finish!: (added: boolean) => void
    const onSubmit = rs.fn(() => new Promise<boolean>(resolve => { finish = resolve }))
    function Harness() {
      const [open, setOpen] = useState(true)
      const [form, setForm] = useState({ ...EMPTY_NODE_FORM, tag: '原节点', server: 'example.com', password: 'test-password' })
      return <>
        <button onClick={() => setOpen(true)}>重新打开</button>
        <NodeModal open={open} nodeType="hysteria2" setNodeType={rs.fn()} form={form} setForm={setForm} loading={false}
          onClose={() => { setOpen(false); setForm({ ...EMPTY_NODE_FORM }) }} onSubmit={onSubmit} onImport={rs.fn()} onDeployVps={rs.fn()} />
      </>
    }
    render(<Harness />)
    await user.click(screen.getByRole('button', { name: '手动填写' }))
    const submit = screen.getByRole('button', { name: '添加 Hysteria2 节点' })
    act(() => { fireEvent.click(submit); fireEvent.click(submit) })
    expect(onSubmit).toHaveBeenCalledTimes(1)
    expect(screen.getByLabelText('节点名称')).toBeDisabled()
    expect(screen.getByLabelText('协议')).toBeDisabled()
    expect(screen.getByLabelText('密码')).toBeDisabled()
    expect(screen.getByLabelText(/SNI/)).toBeDisabled()
    expect(screen.getByRole('button', { name: '粘贴链接' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'VPS 部署' })).toBeDisabled()
    expect(screen.getByRole('status')).toHaveTextContent('关闭窗口不会取消请求')
    await user.click(screen.getByRole('button', { name: '关闭节点对话框' }))
    await user.click(screen.getByRole('button', { name: '重新打开' }))
    await user.type(screen.getByRole('textbox', { name: '节点分享链接' }), '新的草稿')
    await act(async () => { finish(true) })
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: '节点分享链接' })).toHaveValue('新的草稿')
    expect(screen.getByRole('button', { name: '手动填写' })).toBeEnabled()
  })

  it('preserves the failed draft for retry and closes after success', async () => {
    const user = userEvent.setup()
    const onSubmit = rs.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true)
    const onClose = rs.fn()
    renderModal({ onSubmit, onClose, form: { ...EMPTY_NODE_FORM, tag: '重试节点', server: 'example.net', password: 'test-password' } })
    await user.click(screen.getByRole('button', { name: '手动填写' }))
    await user.click(screen.getByRole('button', { name: '添加 Hysteria2 节点' }))
    expect(onClose).not.toHaveBeenCalled()
    expect(screen.getByLabelText('节点名称')).toHaveValue('重试节点')
    expect(screen.getByLabelText('节点名称')).toBeEnabled()
    expect(screen.getByRole('button', { name: '添加 Hysteria2 节点' })).toBeEnabled()
    await user.click(screen.getByRole('button', { name: '添加 Hysteria2 节点' }))
    expect(onSubmit).toHaveBeenCalledTimes(2)
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})

describe('NodeModal link import', () => {
  it('removes only successful request indices and retains invalid lines and backend failures for retry', async () => {
    const user = userEvent.setup()
    const onClose = rs.fn()
    const onImport = rs.fn().mockResolvedValueOnce({ added: [{ index: 1, tag: '同名' }], failed: [{ index: 0, tag: '同名', message: '模拟错误' }] })
      .mockResolvedValueOnce({ added: [{ index: 0, tag: '同名' }], failed: [] })
    renderModal({ onImport, onClose })
    const input = screen.getByRole('textbox', { name: '节点分享链接' })
    const failed = 'trojan://password123@failed.example.com:443#同名'
    const added = 'trojan://password123@added.example.com:443#同名'
    await user.click(input)
    await user.paste(`not-a-link\n${failed}\n${added}`)
    await user.click(screen.getByRole('button', { name: '添加 2 个节点' }))
    expect(input).toHaveValue(`not-a-link\n${failed}`)
    expect(screen.getByRole('status')).toHaveTextContent('同名：模拟错误')
    expect(onClose).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: '添加 1 个节点' }))
    expect(onImport.mock.calls[1][0]).toEqual([expect.objectContaining({ server: 'failed.example.com' })])
    expect(input).toHaveValue('not-a-link')
    expect(onClose).not.toHaveBeenCalled()
  })

  it('guards duplicate import, locks the submitted links, and ignores completion after reopening', async () => {
    const user = userEvent.setup()
    let finish!: (result: BatchNodeResult) => void
    const onImport = rs.fn(() => new Promise<BatchNodeResult>(resolve => { finish = resolve }))
    function Harness() {
      const [open, setOpen] = useState(true)
      return <><button onClick={() => setOpen(true)}>重新打开</button>
        <NodeModal open={open} nodeType="hysteria2" setNodeType={rs.fn()} form={{ ...EMPTY_NODE_FORM }} setForm={rs.fn()}
          loading={false} onClose={() => setOpen(false)} onSubmit={rs.fn()} onImport={onImport} onDeployVps={rs.fn()} /></>
    }
    render(<Harness />)
    await user.click(screen.getByRole('textbox', { name: '节点分享链接' }))
    await user.paste('trojan://password123@example.com:443#x')
    const submit = screen.getByRole('button', { name: '添加 1 个节点' })
    act(() => { fireEvent.click(submit); fireEvent.click(submit) })
    expect(onImport).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('textbox', { name: '节点分享链接' })).toBeDisabled()
    expect(screen.getByRole('button', { name: '手动填写' })).toBeDisabled()
    await user.click(screen.getByRole('button', { name: '关闭节点对话框' }))
    await user.click(screen.getByRole('button', { name: '重新打开' }))
    await user.type(screen.getByRole('textbox', { name: '节点分享链接' }), '新草稿')
    await act(async () => { finish({ added: [{ index: 0, tag: 'x' }], failed: [] }) })
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: '节点分享链接' })).toHaveValue('新草稿')
  })

  it('preserves the full draft when the request fails and closes only on complete success', async () => {
    const user = userEvent.setup()
    const onClose = rs.fn()
    const onImport = rs.fn().mockResolvedValueOnce(undefined).mockResolvedValueOnce({ added: [{ index: 0, tag: 'x' }], failed: [] })
    renderModal({ onImport, onClose })
    const input = screen.getByRole('textbox', { name: '节点分享链接' })
    const link = 'trojan://password123@example.com:443#x'
    await user.click(input)
    await user.paste(link)
    await user.click(screen.getByRole('button', { name: '添加 1 个节点' }))
    expect(input).toHaveValue(link)
    expect(input).toBeEnabled()
    expect(onClose).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: '添加 1 个节点' }))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

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

    expect(screen.getByText(/不支持的链接类型/)).toBeInTheDocument()
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

describe('VPS deployment errors', () => {
  it('keeps the detailed failure visible, preserves inputs and allows dismissal and retry', async () => {
    const user = userEvent.setup()
    const onDeployVps = rs.fn().mockRejectedValueOnce(new Error('SSH 认证被拒绝。请检查 PermitRootLogin 和 PasswordAuthentication。\n\nPermission denied (publickey).')).mockResolvedValueOnce(true)
    renderModal({ onDeployVps })
    await user.click(screen.getByRole('button', { name: 'VPS 部署' }))
    await user.type(screen.getByLabelText('VPS IP 地址'), '203.0.113.10')
    await user.type(screen.getByLabelText('root 密码'), 'rootpass')
    await user.click(screen.getByRole('button', { name: '开始部署' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('PermitRootLogin')
    expect(screen.getByRole('alert')).toHaveTextContent('Permission denied (publickey)')
    expect(screen.getByLabelText('VPS IP 地址')).toHaveValue('203.0.113.10')
    expect(screen.getByLabelText('root 密码')).toHaveValue('rootpass')
    expect(screen.getByRole('button', { name: '开始部署' })).toBeEnabled()
    await user.click(screen.getByRole('button', { name: '关闭部署错误' }))
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '开始部署' }))
    expect(onDeployVps).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })
})
