import { render, screen } from '@testing-library/react'
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
        status={{ running: true, initializing: false }}
        primaryGroup={{ now: 'node-a', all: ['node-a'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={onTestDelay}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
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
        status={{ running: true, initializing: false }}
        primaryGroup={{ now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={onTestDelay}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
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
        status={{ running: true, initializing: false }}
        primaryGroup={{ now: 'node-a', all: ['node-a', 'node-b'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{ 'node-b': true }}
        testingGroup=""
        onTestDelay={vi.fn()}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
        onOpenAddNode={vi.fn()}
      />,
    )

    const switchButton = screen.getByRole('button', { name: '切换到 node-b' })
    expect(switchButton).toBeEnabled()
    expect(screen.getByRole('button', { name: '测试 node-b 延迟' })).toBeDisabled()

    await user.click(switchButton)
    expect(onSwitchProxy).toHaveBeenCalledWith('proxy', 'node-b')
  })
})
