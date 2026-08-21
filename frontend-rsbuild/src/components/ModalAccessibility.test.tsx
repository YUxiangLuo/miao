import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, rs } from '@rstest/core'
import { connectionMock, statusMock } from '../testFixtures'
import { ConfirmModal } from './ConfirmModal'
import { NodeModal } from './NodeModal'
import { ConnectionsModal } from './ConnectionsModal'
import { EMPTY_NODE_FORM } from '../utils'

describe('modal accessibility', () => {
  it('exposes a labelled dialog and closes it with Escape', async () => {
    const user = userEvent.setup()
    const onCancel = rs.fn()

    render(
      <ConfirmModal
        open
        title="删除节点"
        message="确定删除吗？"
        onCancel={onCancel}
        onConfirm={rs.fn()}
      />,
    )

    const dialog = screen.getByRole('dialog', { name: '删除节点' })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    // 初始焦点应在确认按钮上，Enter 即确认，不会误触关闭
    expect(screen.getByRole('button', { name: '确认' })).toHaveFocus()

    await user.keyboard('{Escape}')
    expect(onCancel).toHaveBeenCalledTimes(1)
  })

  it('uses protected password inputs in the node dialog', async () => {
    const user = userEvent.setup()
    const onClose = rs.fn()

    render(
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
      />,
    )

    expect(screen.getByRole('dialog', { name: '添加节点' })).toBeInTheDocument()
    // 默认是链接导入模式,手动表单需切换
    expect(screen.getByRole('textbox', { name: '节点分享链接' })).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: '手动填写' }))
    expect(screen.getByLabelText('密码')).toHaveAttribute('type', 'password')

    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('exposes a labelled connections dialog with labelled connection rows', async () => {
    const user = userEvent.setup()
    const onClose = rs.fn()

    render(
      <ConnectionsModal
        open
        status={statusMock({ running: true })}
        data={{
          uploadTotal: 0,
          downloadTotal: 0,
          connections: [connectionMock({
            metadata: {
              host: 'example.com',
              destinationPort: '443',
              network: 'tcp',
              sourceIP: '127.0.0.1',
            },
          })],
        }}
        loading={false}
        error=""
        onClose={onClose}
      />,
    )

    expect(screen.getByRole('dialog', { name: '链接统计' })).toBeInTheDocument()
    // 链接行不再展开，每条连接以域名命名直接可定位
    expect(screen.getByLabelText('example.com 链接')).toBeInTheDocument()

    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
