import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ConfirmModal } from './ConfirmModal.jsx'
import { NodeModal } from './NodeModal.jsx'
import { ConnectionsModal } from './ConnectionsModal.jsx'
import { EMPTY_NODE_FORM } from '../utils.js'

describe('modal accessibility', () => {
  it('exposes a labelled dialog and closes it with Escape', async () => {
    const user = userEvent.setup()
    const onCancel = vi.fn()

    render(
      <ConfirmModal
        open
        title="删除节点"
        message="确定删除吗？"
        onCancel={onCancel}
        onConfirm={vi.fn()}
      />,
    )

    const dialog = screen.getByRole('dialog', { name: '删除节点' })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(screen.getByRole('button', { name: '关闭确认对话框' })).toHaveFocus()

    await user.keyboard('{Escape}')
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('uses protected password inputs in the node dialog', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()

    render(
      <NodeModal
        open
        nodeType="hysteria2"
        setNodeType={vi.fn()}
        form={{
          ...EMPTY_NODE_FORM,
          tag: 'node',
          server: 'node.example.com',
          password: 'password123',
        }}
        setForm={vi.fn()}
        loading={false}
        onClose={onClose}
        onSubmit={vi.fn()}
      />,
    )

    expect(screen.getByRole('dialog', { name: '添加节点' })).toBeInTheDocument()
    expect(screen.getByLabelText('密码')).toHaveAttribute('type', 'password')

    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('keeps connection details and close actions as sibling buttons', async () => {
    const user = userEvent.setup()

    render(
      <ConnectionsModal
        open
        status={{ running: true }}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [{
            id: 'connection-1',
            upload: 0,
            download: 0,
            chains: ['proxy'],
            rule: 'Match',
            metadata: {
              host: 'example.com',
              destinationPort: 443,
              network: 'tcp',
              sourceIP: '127.0.0.1',
            },
          }],
        }}
        loading={false}
        error=""
        onClose={vi.fn()}
        onRefresh={vi.fn()}
        onCloseConnection={vi.fn()}
        showToast={vi.fn()}
      />,
    )

    const detailsButton = screen.getByRole('button', {
      name: '查看连接 example.com:443 的详情',
    })
    const closeButton = screen.getByRole('button', {
      name: '关闭连接 example.com:443',
    })
    expect(detailsButton.contains(closeButton)).toBe(false)

    await user.click(detailsButton)
    expect(screen.getByText('连接详情')).toBeInTheDocument()
  })
})
