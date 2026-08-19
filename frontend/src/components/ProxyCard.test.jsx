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

  it('switches an inactive tile', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()

    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'manual' }}
        primaryGroup={{ now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={vi.fn()}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={vi.fn()}
        onOpenAddNode={vi.fn()}
      />,
    )

    // 当前节点横幅已迁至顶栏，卡片内不再提供「测试当前节点」入口
    expect(screen.queryByRole('button', { name: /测试当前节点/ })).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '切换到 node-b' }))
    expect(onSwitchProxy).toHaveBeenCalledWith('proxy', 'node-b')
  })

  it('shows the protocol name when nodeProtocols has an entry', () => {
    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'manual' }}
        primaryGroup={{ now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        nodeProtocols={{ 'node-a': 'Hysteria2' }}
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

    expect(screen.getByText('Hysteria2')).toBeInTheDocument()
    // 无协议数据的节点不渲染协议行
    expect(screen.getByRole('button', { name: '切换到 node-b' }).textContent).not.toContain('Hysteria2')
  })

  it('keeps switching available while a node delay test is running', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()

    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'manual' }}
        primaryGroup={{ now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
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

  it('places the node-select dropdown in the header controls (right cluster)', () => {
    render(
      <ProxyCard
        status={{ running: true, initializing: false, node_select: 'fastest_hk' }}
        primaryGroup={{ now: '香港-01', all: ['香港-01'] }}
        primaryGroupName="proxy"
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
    const title = screen.getByText('节点列表')
    expect(title).toBeInTheDocument()
    // select 属于头部右侧控件组，不在标题簇内（title-wrap 占满剩余宽度把控件推到右边）
    const header = title.closest('.section-header')
    expect(header).not.toBeNull()
    expect(header.contains(select)).toBe(true)
    expect(title.closest('.section-title-wrap').contains(select)).toBe(false)
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
