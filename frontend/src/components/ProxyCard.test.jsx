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

    const switchButton = screen.getByRole('button', { name: '切换到 node-a' })
    const delayButton = screen.getByRole('button', { name: '测试 node-a 延迟' })
    expect(switchButton.contains(delayButton)).toBe(false)

    await user.click(switchButton)
    expect(onSwitchProxy).toHaveBeenCalledWith('proxy', 'node-a')

    await user.click(delayButton)
    expect(onTestDelay).toHaveBeenCalledWith('node-a')
  })
})
