import { render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
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

    expect(screen.getByRole('dialog', { name: '添加节点' })).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: '节点分享链接' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '手动配置' }))
    expect(screen.getByLabelText('密码')).toHaveAttribute('type', 'password')

    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('keeps dialog focus stable across live prop updates and cleanly refocuses on reopen', async () => {
    const user = userEvent.setup()
    const [state, setState] = createSignal({ open: true, title: '标题 A', message: '内容 A' })
    const onCancel = rs.fn()

    render(() => (
      <ConfirmModal
        open={state().open}
        title={state().title}
        message={state().message}
        onCancel={onCancel}
        onConfirm={rs.fn()}
      />
    ))
    flush()

    const cancelBtn = screen.getByRole('button', { name: '取消' })
    cancelBtn.focus()
    expect(cancelBtn).toHaveFocus()

    // Updating state object while `open` stays true must not tear down or steal focus.
    setState({ open: true, title: '标题 B', message: '内容 B' })
    flush()
    expect(screen.getByRole('dialog', { name: '标题 B' })).toBeInTheDocument()
    expect(cancelBtn).toHaveFocus()

    // Closing and reopening focuses the newly connected dialog cleanly.
    setState({ open: false, title: '标题 B', message: '内容 B' })
    flush()
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()

    setState({ open: true, title: '标题 C', message: '内容 C' })
    flush()
    expect(screen.getByRole('button', { name: '确认' })).toHaveFocus()

    await user.keyboard('{Escape}')
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('does not reset the active NodeModal tab when unrelated props update while open', async () => {
    const user = userEvent.setup()
    const [modalState, setModalState] = createSignal({ open: true, initialMode: 'link' as const, loading: false })

    render(() => (
      <NodeModal
        open={modalState().open}
        initialMode={modalState().initialMode}
        nodeType="hysteria2"
        setNodeType={rs.fn()}
        form={EMPTY_NODE_FORM}
        setForm={rs.fn()}
        loading={modalState().loading}
        onClose={rs.fn()}
        onSubmit={rs.fn()}
        onImport={rs.fn()}
        onDeployVps={rs.fn()}
      />
    ))
    flush()

    await user.click(screen.getByRole('button', { name: '手动配置' }))
    expect(screen.getByLabelText('密码')).toBeInTheDocument()

    setModalState({ open: true, initialMode: 'link', loading: true })
    flush()
    expect(screen.getByLabelText('密码')).toBeInTheDocument()
  })
})
