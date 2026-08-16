import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { TopBar } from './TopBar.jsx'

const running = { running: true, initializing: false }

describe('TopBar version chip', () => {
  it('does not offer in-app upgrade when the platform cannot replace the binary', () => {
    const onUpgradeClick = vi.fn()
    render(
      <TopBar
        status={running}
        versionInfo={{
          current: 'v0.31.0',
          latest: 'v0.32.0',
          has_update: true,
          upgrade_supported: false,
        }}
        upgrading={false}
        onUpgradeClick={onUpgradeClick}
      />,
    )

    expect(screen.queryByRole('button', { name: /v0/ })).not.toBeInTheDocument()
    expect(screen.getByText('v0.31.0')).toBeInTheDocument()
    expect(onUpgradeClick).not.toHaveBeenCalled()
  })

  it('keeps the upgrade button when self-update is supported', async () => {
    const user = userEvent.setup()
    const onUpgradeClick = vi.fn()
    render(
      <TopBar
        status={running}
        versionInfo={{
          current: 'v0.31.0',
          latest: 'v0.32.0',
          has_update: true,
          upgrade_supported: true,
        }}
        upgrading={false}
        onUpgradeClick={onUpgradeClick}
      />,
    )

    await user.click(screen.getByRole('button', { name: /v0.32.0/ }))
    expect(onUpgradeClick).toHaveBeenCalledTimes(1)
  })
})
