import { useState } from 'react'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ConfirmModal, NodeModal } from './modals.jsx'
import { EMPTY_NODE_FORM, nodeTypeDefaults } from '../utils.js'

function ConfirmModalHarness() {
  const [open, setOpen] = useState(false)

  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>删除节点</button>
      <ConfirmModal
        open={open}
        title="删除节点"
        message="确定删除吗？"
        onCancel={() => setOpen(false)}
        onConfirm={() => setOpen(false)}
      />
    </>
  )
}

describe('modal accessibility', () => {
  it('traps focus, closes with Escape, and restores focus', async () => {
    const user = userEvent.setup()
    render(<ConfirmModalHarness />)

    const trigger = screen.getByRole('button', { name: '删除节点' })
    await user.click(trigger)

    expect(screen.getByRole('dialog', { name: '删除节点' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '取消' })).toHaveFocus()

    await user.tab()
    expect(screen.getByRole('button', { name: '确认' })).toHaveFocus()

    await user.tab()
    expect(screen.getByRole('button', { name: '关闭确认对话框' })).toHaveFocus()

    await user.keyboard('{Escape}')

    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(trigger).toHaveFocus()
  })

  it('uses protected inputs for node secrets', () => {
    const form = {
      ...EMPTY_NODE_FORM,
      ...nodeTypeDefaults('hysteria2'),
      obfs_type: 'salamander',
    }

    render(
      <NodeModal
        open
        nodeType="hysteria2"
        setNodeType={vi.fn()}
        form={form}
        setForm={vi.fn()}
        loading={false}
        onClose={vi.fn()}
        onSubmit={vi.fn()}
      />
    )

    expect(screen.getByRole('dialog', { name: '添加节点' })).toBeInTheDocument()
    expect(screen.getByLabelText('密码')).toHaveAttribute('type', 'password')
    expect(screen.getByLabelText('混淆密码')).toHaveAttribute('type', 'password')
  })
})
