import { act, renderHook } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { POLL_INTERVAL } from '../utils.js'
import { usePolling } from './usePolling.js'

describe('usePolling', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('waits for the current run before scheduling the next one', async () => {
    let finishFirstRun
    const task = vi
      .fn()
      .mockImplementationOnce(() => new Promise((resolve) => {
        finishFirstRun = resolve
      }))
      .mockResolvedValue(undefined)

    renderHook(() => usePolling([task]))
    await act(async () => Promise.resolve())

    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_INTERVAL * 3)
    })
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      finishFirstRun()
      await Promise.resolve()
    })

    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_INTERVAL - 1)
    })
    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1)
    })
    expect(task).toHaveBeenCalledTimes(2)
  })

  it('continues polling after an asynchronous task rejection', async () => {
    const task = vi.fn().mockRejectedValue(new Error('temporary failure'))

    renderHook(() => usePolling([task]))
    await act(async () => Promise.resolve())

    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_INTERVAL)
    })
    expect(task).toHaveBeenCalledTimes(2)
  })

  it('supports delaying the first run', async () => {
    const task = vi.fn().mockResolvedValue(undefined)

    renderHook(() => usePolling([task], true, false))
    await act(async () => Promise.resolve())

    expect(task).not.toHaveBeenCalled()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(POLL_INTERVAL)
    })
    expect(task).toHaveBeenCalledTimes(1)
  })

  it('recovers when a polling task never settles', async () => {
    const timeout = 1000
    let activeTasks = 0
    let maxActiveTasks = 0
    const task = vi.fn((signal) => new Promise((resolve) => {
      activeTasks += 1
      maxActiveTasks = Math.max(maxActiveTasks, activeTasks)
      signal.addEventListener('abort', () => {
        activeTasks -= 1
        resolve()
      }, { once: true })
    }))

    renderHook(() => usePolling([task], true, true, timeout))
    await act(async () => Promise.resolve())

    expect(task).toHaveBeenCalledTimes(1)

    await act(async () => {
      await vi.advanceTimersByTimeAsync(timeout + POLL_INTERVAL)
    })
    expect(task).toHaveBeenCalledTimes(2)
    expect(maxActiveTasks).toBe(1)
  })
})
