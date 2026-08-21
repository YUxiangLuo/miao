import { act, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, rs } from '@rstest/core'
import { ProxyCard } from './ProxyCard'
import { ARRIVE_MS } from '../tokens'
import type { StatusData } from '../types/api'
import type { ClashProxy } from '../types/clash'

function statusMock(node_select: StatusData['node_select']): StatusData {
  return {
    running: true,
    ready: true,
    phase: 'ready',
    initializing: false,
    node_select,
    route_mode: 'rule',
    vps_supported: true,
    platform: 'linux',
    mcp: false,
  }
}

describe('ProxyCard accessibility', () => {
  it('renders switching and delay testing as sibling buttons', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = rs.fn()
    const onTestDelay = rs.fn()

    render(
      <ProxyCard
        status={statusMock('manual')}
        primaryGroup={{ type: 'Selector', now: 'node-a', all: ['node-a'] }}
        primaryGroupName="proxy"
        switchingNode=""
        nodeSelectPending={false}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={onTestDelay}
        onTestGroupDelays={rs.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={rs.fn()}
        onOpenAddNode={rs.fn()}
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
    const onSwitchProxy = rs.fn()

    render(
      <ProxyCard
        status={statusMock('manual')}
        primaryGroup={{ type: 'Selector', now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        switchingNode=""
        nodeSelectPending={false}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={rs.fn()}
        onTestGroupDelays={rs.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={rs.fn()}
        onOpenAddNode={rs.fn()}
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
        status={statusMock('manual')}
        primaryGroup={{ type: 'Selector', now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        switchingNode=""
        nodeSelectPending={false}
        nodeProtocols={{ 'node-a': 'Hysteria2' }}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={rs.fn()}
        onTestGroupDelays={rs.fn()}
        onSwitchProxy={rs.fn()}
        onSetNodeSelect={rs.fn()}
        onOpenAddNode={rs.fn()}
      />,
    )

    expect(screen.getByText('Hysteria2')).toBeInTheDocument()
    // 无协议数据的节点不渲染协议行
    expect(screen.getByRole('button', { name: '切换到 node-b' }).textContent).not.toContain('Hysteria2')
  })

  it('keeps switching available while a node delay test is running', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = rs.fn()

    render(
      <ProxyCard
        status={statusMock('manual')}
        primaryGroup={{ type: 'Selector', now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        switchingNode=""
        nodeSelectPending={false}
        delays={{}}
        testingNodes={{ 'node-b': true }}
        testingGroup=""
        onTestDelay={rs.fn()}
        onTestGroupDelays={rs.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={rs.fn()}
        onOpenAddNode={rs.fn()}
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
        status={statusMock('fastest_hk')}
        primaryGroup={{ type: 'Selector', now: '香港-01', all: ['香港-01'] }}
        primaryGroupName="proxy"
        switchingNode=""
        nodeSelectPending={false}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={rs.fn()}
        onTestGroupDelays={rs.fn()}
        onSwitchProxy={rs.fn()}
        onSetNodeSelect={rs.fn()}
        onOpenAddNode={rs.fn()}
      />,
    )

    const select = screen.getByRole('combobox', { name: '节点选择' })
    expect(select).toHaveValue('fastest_hk')
    const title = screen.getByText('节点列表')
    expect(title).toBeInTheDocument()
    // select 属于头部右侧控件组，不在标题簇内（title-wrap 占满剩余宽度把控件推到右边）
    const header = title.closest('.section-header')
    expect(header).not.toBeNull()
    if (!header) throw new Error('section-header not found')
    expect(header.contains(select)).toBe(true)
    const titleWrap = title.closest('.section-title-wrap')
    if (!titleWrap) throw new Error('section-title-wrap not found')
    expect(titleWrap.contains(select)).toBe(false)
  })

  it('disables tile switching in fastest mode but still tests delay', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = rs.fn()
    const onTestDelay = rs.fn()

    render(
      <ProxyCard
        status={statusMock('fastest_jp')}
        primaryGroup={{ type: 'Selector', now: '日本-01', all: ['日本-01', '日本-02'] }}
        primaryGroupName="proxy"
        switchingNode=""
        nodeSelectPending={false}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={onTestDelay}
        onTestGroupDelays={rs.fn()}
        onSwitchProxy={onSwitchProxy}
        onSetNodeSelect={rs.fn()}
        onOpenAddNode={rs.fn()}
      />,
    )

    expect(screen.getByRole('button', { name: '切换到 日本-02' })).toBeDisabled()
    await user.click(screen.getByRole('button', { name: '测试 日本-02 延迟' }))
    expect(onTestDelay).toHaveBeenCalledWith('日本-02')
    expect(onSwitchProxy).not.toHaveBeenCalled()
  })

  it('keeps the chosen node-select value while the change is in flight', async () => {
    const user = userEvent.setup()
    let settle: (() => void) | undefined
    const onSetNodeSelect = rs.fn(
      () => new Promise<void>((resolve) => { settle = resolve }),
    )

    render(
      <ProxyCard
        status={statusMock('manual')}
        primaryGroup={{ type: 'Selector', now: 'node-a', all: ['node-a'] }}
        primaryGroupName="proxy"
        switchingNode=""
        nodeSelectPending={false}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={rs.fn()}
        onTestGroupDelays={rs.fn()}
        onSwitchProxy={rs.fn()}
        onSetNodeSelect={onSetNodeSelect}
        onOpenAddNode={rs.fn()}
      />,
    )

    const select = screen.getByRole('combobox', { name: '节点选择' })
    await user.selectOptions(select, 'fastest_hk')

    // 请求在途期间停在用户选择上，不弹回 status 里的旧值
    expect(onSetNodeSelect).toHaveBeenCalledWith('fastest_hk')
    expect(select).toHaveValue('fastest_hk')

    // 处理器结束后回到服务端真值（本例 status 未变，等效失败回弹）
    await act(async () => { settle?.() })
    expect(select).toHaveValue('manual')
  })
})

describe('ProxyCard switch arrival animation', () => {
  const baseProps = {
    primaryGroupName: 'proxy',
    switchingNode: '',
    nodeSelectPending: false,
    delays: {},
    testingNodes: {},
    testingGroup: '',
    onTestDelay: rs.fn(),
    onTestGroupDelays: rs.fn(),
    onSwitchProxy: rs.fn(),
    onSetNodeSelect: rs.fn(),
    onOpenAddNode: rs.fn(),
  }

  function groupMock(now: string, all: string[]): ClashProxy {
    return { type: 'Selector', now, all }
  }

  function renderCard(primaryGroup: ClashProxy | null) {
    return render(
      <ProxyCard {...baseProps} status={statusMock('manual')} primaryGroup={primaryGroup} />,
    )
  }

  function rerenderCard(
    rerender: (ui: React.ReactElement) => void,
    primaryGroup: ClashProxy | null,
  ) {
    rerender(
      <ProxyCard {...baseProps} status={statusMock('manual')} primaryGroup={primaryGroup} />,
    )
  }

  const activeTile = (name: string) =>
    screen.getByRole('button', { name: `当前节点 ${name}` }).closest('.proxy-tile')

  beforeEach(() => {
    rs.useFakeTimers()
  })

  afterEach(() => {
    rs.useRealTimers()
  })

  it('marks the newly active tile with arrive after a switch, then clears it', () => {
    const { container, rerender } = renderCard(groupMock('node-a', ['node-a', 'node-b']))

    // 自动/手动切换同路径：now 变化后新选中 tile 拿到 .arrive
    rerenderCard(rerender, groupMock('node-b', ['node-a', 'node-b']))
    expect(activeTile('node-b')).toHaveClass('arrive')
    expect(container.querySelectorAll('.proxy-tile.arrive')).toHaveLength(1)

    // ARRIVE_MS 后移除，交棒给 CSS 的 tileGlow 呼吸
    act(() => {
      rs.advanceTimersByTime(ARRIVE_MS)
    })
    expect(activeTile('node-b')).not.toHaveClass('arrive')
  })

  it('does not mark arrival on first render', () => {
    const { container } = renderCard(groupMock('node-a', ['node-a', 'node-b']))
    expect(activeTile('node-a')).not.toHaveClass('arrive')
    expect(container.querySelectorAll('.proxy-tile.arrive')).toHaveLength(0)
  })

  it('ignores list rebuilds where the previous node is gone', () => {
    // 刷新订阅后候选列表重建：now 落在全新节点上不算切换
    const { container, rerender } = renderCard(groupMock('node-a', ['node-a', 'node-b']))
    rerenderCard(rerender, groupMock('node-c', ['node-c', 'node-d']))
    expect(container.querySelectorAll('.proxy-tile.arrive')).toHaveLength(0)
  })

  it('keeps the pending arrival across re-polls with the same now', () => {
    // 轮询每次产生新的 all 数组身份，effect 会重跑——但不许碰未决的脉冲定时器
    const { rerender } = renderCard(groupMock('node-a', ['node-a', 'node-b']))
    rerenderCard(rerender, groupMock('node-b', ['node-a', 'node-b']))
    expect(activeTile('node-b')).toHaveClass('arrive')

    act(() => {
      rs.advanceTimersByTime(ARRIVE_MS / 2)
    })
    rerenderCard(rerender, groupMock('node-b', ['node-a', 'node-b']))
    act(() => {
      rs.advanceTimersByTime(ARRIVE_MS / 2 - 1)
    })
    expect(activeTile('node-b')).toHaveClass('arrive')

    act(() => {
      rs.advanceTimersByTime(1)
    })
    expect(activeTile('node-b')).not.toHaveClass('arrive')
  })

  it('ignores transient proxy wipeouts (poll failure recovery)', () => {
    // 轮询失败时 proxies 被清空为 null，恢复后 now「变回」原节点不应误报
    const { container, rerender } = renderCard(groupMock('node-a', ['node-a', 'node-b']))
    rerenderCard(rerender, null)
    rerenderCard(rerender, groupMock('node-a', ['node-a', 'node-b']))
    expect(container.querySelectorAll('.proxy-tile.arrive')).toHaveLength(0)
  })
})
