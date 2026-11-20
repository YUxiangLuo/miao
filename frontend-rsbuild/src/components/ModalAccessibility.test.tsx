import { render, screen } from '@solidjs/testing-library'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { ConfirmModal } from './ConfirmModal'
import { NodeModal } from './NodeModal'
import { EMPTY_NODE_FORM } from '../utils'

describe('modal accessibility', () => {
  it('exposes a labelled dialog and closes it with Escape', async () => {
    const user = userEvent.setup()
    const onCancel = rs.fn()

    render(() => (
      <ConfirmModal
        open
        title="删除节点"
        message="确定删除吗？"
        onCancel={onCancel}
        onConfirm={rs.fn()}
      />
    ))

    const dialog = screen.getByRole('dialog', { name: '删除节点' })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(screen.getByRole('button', { name: '确认' })).toHaveFocus()

    await user.keyboard('{Escape}')
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('uses protected password inputs in the node dialog', async () => {
    const user = userEvent.setup()
    const onClose = rs.fn()

    render(() => (
      <NodeModal
        open
        nodeType="hysteria2"
        setNodeType={rs.fn()}
        form={{
          ...EMPTY_NODE_FORM,
          tag: 'node',
          server: 'node.example.com',
          password: 'password123',
        }}
        setForm={rs.fn()}
        loading={false}
        onClose={onClose}
        onSubmit={rs.fn()}
        onImport={rs.fn()}
        onDeployVps={rs.fn()}
      />
    ))

    expect(screen.getByRole('dialog', { name: '新增独享专线 / 节点' })).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: '节点分享链接' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '手动 / 静态住宅' }))
    expect(screen.getByLabelText('密码')).toHaveAttribute('type', 'password')

    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
