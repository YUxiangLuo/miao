import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import { ConnectivityCard } from './ConnectivityCard.jsx'

describe('ConnectivityCard', () => {
  it('keeps the stop action available while a test is running', async () => {
    const user = userEvent.setup()
    const onStopTest = vi.fn()

    render(
      <ConnectivityCard
        connectivityResults={{}}
        testingConnectivity
        currentTestingSite="Google"
        status={{ initializing: false }}
        onTestAll={vi.fn()}
        onStopTest={onStopTest}
        onTestSingleSite={vi.fn()}
      />
    )

    const stopButton = screen.getByRole('button', { name: '停止测试' })
    expect(stopButton).toBeEnabled()

    await user.click(stopButton)
    expect(onStopTest).toHaveBeenCalledTimes(1)
  })
})
