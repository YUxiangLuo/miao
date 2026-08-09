import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { POLL_INTERVAL } from '../utils.js'
import { usePolling } from './usePolling.js'

describe('usePolling', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('does not overlap polling rounds while a task is still running', async () => {
    let finishFirstRun
    const task = vi.fn(() => new Promise((resolve) => {
      finishFirstRun = resolve
    }))

    renderHook(() => usePolling([task], true))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    act(() => vi.advanceTimersByTime(POLL_INTERVAL * 2))
    await act(async () => Promise.resolve())
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      finishFirstRun()
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
})
