import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ProxyCard } from './ProxyCard.jsx'

describe('ProxyCard', () => {
  it('allows keyboard users to switch nodes', async () => {
    const user = userEvent.setup()
    const onSwitchProxy = vi.fn()

    render(
      <ProxyCard
        status={{ running: true }}
        primaryGroup={{ now: 'Tokyo', all: ['Tokyo', 'Osaka'] }}
        primaryGroupName="proxy"
        currentNodeMeta={null}
        delays={{}}
        testingNodes={{}}
        testingGroup=""
        onTestDelay={vi.fn()}
        onTestGroupDelays={vi.fn()}
        onSwitchProxy={onSwitchProxy}
        onOpenAddNode={vi.fn()}
      />
    )

    expect(screen.getByRole('button', { name: '当前节点 Tokyo' })).toHaveAttribute('aria-pressed', 'true')

    const osakaButton = screen.getByRole('button', { name: '切换到节点 Osaka' })
    osakaButton.focus()
    await user.keyboard('{Enter}')

    expect(onSwitchProxy).toHaveBeenCalledWith('proxy', 'Osaka')
  })
})
