import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ProxyCard } from './ProxyCard.jsx'

describe('ProxyCard accessibility', () => {
  it('renders switching and delay testing as sibling buttons', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()
    const onTestDelay = vi.fn()

    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'manual' }}
        primaryGroup={{ now: 'node-a', all: ['node-a'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={onTestDelay}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={vi.fn()}
        onOpenAddNode={vi.fn()}
      />,
    )

    const switchButton = screen.getByRole('button', { name: '当前节点 node-a' })
    const delayButton = screen.getByRole('button', { name: '测试 node-a 延迟' })
    expect(switchButton.contains(delayButton)).toBe(false)
    expect(switchButton).toBeDisabled()

    await user.click(delayButton)
    expect(onSwitchProxy).not.toHaveBeenCalled()
    expect(onTestDelay).toHaveBeenCalledWith('node-a')
  })

  it('tests the current node from the banner and still switches an inactive tile', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()
    const onTestDelay = vi.fn()

    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'manual' }}
        primaryGroup={{ now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={onTestDelay}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={vi.fn()}
        onOpenAddNode={vi.fn()}
      />,
    )

    await user.click(screen.getByRole('button', { name: '测试当前节点 node-a 延迟' }))
    expect(onTestDelay).toHaveBeenCalledWith('node-a')

    await user.click(screen.getByRole('button', { name: '切换到 node-b' }))
    expect(onSwitchProxy).toHaveBeenCalledWith('proxy', 'node-b')
  })

  it('keeps switching available while a node delay test is running', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()

    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'manual' }}
        primaryGroup={{ now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{ 'node-b': true }}
        testingGroup=""
        onTestDelay={vi.fn()}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={vi.fn()}
        onOpenAddNode={vi.fn()}
      />,
    )

    const switchButton = screen.getByRole('button', { name: '切换到 node-b' })
    expect(switchButton).toBeEnabled()
    expect(screen.getByRole('button', { name: '测试 node-b 延迟' })).toBeDisabled()

    await user.click(switchButton)
    expect(onSwitchProxy).toHaveBeenCalledWith('proxy', 'node-b')
  })

  it('places the node-select dropdown next to the title', () => {
    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'fastest_hk' }}
        primaryGroup={{ now: '香港-01', all: ['香港-01'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={vi.fn()}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={vi.fn()}
        onSetNodeSelect={vi.fn()}
        onOpenAddNode={vi.fn()}
      />,
    )

    const select = screen.getByRole('combobox', { name: '节点选择' })
    expect(select).toHaveValue('fastest_hk')
    expect(screen.getByText('节点列表')).toBeInTheDocument()
  })

  it('disables tile switching in fastest mode but still tests delay', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()
    const onTestDelay = vi.fn()

    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'fastest_jp' }}
        primaryGroup={{ now: '日本-01', all: ['日本-01', '日本-02'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={onTestDelay}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={vi.fn()}
        onOpenAddNode={vi.fn()}
      />,
    )

    expect(screen.getByRole('button', { name: '切换到 日本-02' })).toBeDisabled()
    await user.click(screen.getByRole('button', { name: '测试 日本-02 延迟' }))
    expect(onTestDelay).toHaveBeenCalledWith('日本-02')
    expect(onSwitchProxy).not.toHaveBeenCalled()
  })

  it('keeps the chosen node-select value while the change is in flight', async () => {
    const user = userEvent.setup()
    let settle
    const onSetNodeSelect = vi.fn(
      () => new Promise((resolve) => { settle = resolve }),
    )

    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'manual' }}
        primaryGroup={{ now: 'node-a', all: ['node-a'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={vi.fn()}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={vi.fn()}
        onSetNodeSelect={onSetNodeSelect}
        onOpenAddNode={vi.fn()}
      />,
    )

    const select = screen.getByRole('combobox', { name: '节点选择' })
    await user.selectOptions(select, 'fastest_hk')

    // 请求在途期间停在用户选择上，不弹回 status 里的旧值
    expect(onSetNodeSelect).toHaveBeenCalledWith('fastest_hk')
    expect(select).toHaveValue('fastest_hk')

    // 处理器结束后回到服务端真值（本例 status 未变，等效失败回弹）
    await act(async () => { settle() })
    expect(select).toHaveValue('manual')
  })
})
