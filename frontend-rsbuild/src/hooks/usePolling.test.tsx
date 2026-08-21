import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, rs } from '@rstest/core'
import { POLL_INTERVAL } from '../utils'
import { usePolling } from './usePolling'

describe('usePolling', () => {
  beforeEach(() => {
    rs.useFakeTimers()
  })
  afterEach(() => {
    rs.useRealTimers()
  })

  it('does not overlap polling rounds while a task is still running', async () => {
    let finishFirstRun: (() => void) | undefined
    const task = rs.fn(() => new Promise<void>((resolve) => {
      finishFirstRun = resolve
    }))

    renderHook(() => usePolling([task], true))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    act(() => rs.advanceTimersByTime(POLL_INTERVAL * 2))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      finishFirstRun?.()
      await Promise.resolve()
    })
    act(() => rs.advanceTimersByTime(POLL_INTERVAL))
    await act(async () => Promise.resolve())

    expect(task).toHaveBeenCalledTimes(2)
  })

  it('continues polling independent tasks when one task is stuck', async () => {
    const stuckTask = rs.fn(() => new Promise(() => {}))
    const healthyTask = rs.fn(() => Promise.resolve())

    renderHook(() => usePolling([stuckTask, healthyTask], true))
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(stuckTask).toHaveBeenCalledTimes(1)
    expect(healthyTask).toHaveBeenCalledTimes(1)

    act(() => rs.advanceTimersByTime(POLL_INTERVAL))
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(stuckTask).toHaveBeenCalledTimes(1)
    expect(healthyTask).toHaveBeenCalledTimes(2)
  })

  it('runs immediately and adopts a changed interval', async () => {
    const task = rs.fn(() => Promise.resolve())
    const customInterval = 1_000
    const { rerender } = renderHook(
      ({ interval }) => usePolling([task], true, interval),
      { initialProps: { interval: POLL_INTERVAL } },
    )
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    rerender({ interval: customInterval })
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(2)

    act(() => rs.advanceTimersByTime(customInterval))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(3)
  })
})
