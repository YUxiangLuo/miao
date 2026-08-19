import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { POLL_INTERVAL, POLL_INTERVAL_STARTUP } from '../utils'
import { usePolling } from './usePolling'

describe('usePolling', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('does not overlap polling rounds while a task is still running', async () => {
    let finishFirstRun: (() => void) | undefined
    const task = vi.fn(() => new Promise<void>((resolve) => {
      finishFirstRun = resolve
    }))

    renderHook(() => usePolling([task], true))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    act(() => vi.advanceTimersByTime(POLL_INTERVAL * 2))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      finishFirstRun?.()
      await Promise.resolve()
    })
    act(() => vi.advanceTimersByTime(POLL_INTERVAL))
    await act(async () => Promise.resolve())

    expect(task).toHaveBeenCalledTimes(2)
  })

  it('continues polling independent tasks when one task is stuck', async () => {
    const stuckTask = vi.fn(() => new Promise(() => {}))
    const healthyTask = vi.fn(() => Promise.resolve())

    renderHook(() => usePolling([stuckTask, healthyTask], true))
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(stuckTask).toHaveBeenCalledTimes(1)
    expect(healthyTask).toHaveBeenCalledTimes(1)

    act(() => vi.advanceTimersByTime(POLL_INTERVAL))
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(stuckTask).toHaveBeenCalledTimes(1)
    expect(healthyTask).toHaveBeenCalledTimes(2)
  })

  it('runs immediately and polls faster when the interval shrinks (startup acceleration)', async () => {
    const task = vi.fn(() => Promise.resolve())
    const { rerender } = renderHook(
      ({ interval }) => usePolling([task], true, interval),
      { initialProps: { interval: POLL_INTERVAL } },
    )
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    // 间隔切到启动档：立即补跑一轮，并按新间隔轮询
    rerender({ interval: POLL_INTERVAL_STARTUP })
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(2)

    act(() => vi.advanceTimersByTime(POLL_INTERVAL_STARTUP))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(3)
  })
})
