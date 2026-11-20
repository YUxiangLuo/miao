import { renderHook } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
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
    flush()
    await Promise.resolve()
    expect(task).toHaveBeenCalledTimes(1)

    rs.advanceTimersByTime(POLL_INTERVAL * 2)
    await Promise.resolve()
    expect(task).toHaveBeenCalledTimes(1)

    finishFirstRun?.()
    await Promise.resolve()
    await Promise.resolve()
    rs.advanceTimersByTime(POLL_INTERVAL)
    await Promise.resolve()

    expect(task).toHaveBeenCalledTimes(2)
  })

  it('continues polling independent tasks when one task is stuck', async () => {
    const stuckTask = rs.fn(() => new Promise(() => {}))
    const healthyTask = rs.fn(() => Promise.resolve())

    renderHook(() => usePolling([stuckTask, healthyTask], true))
    flush()
    await Promise.resolve()
    await Promise.resolve()

    expect(stuckTask).toHaveBeenCalledTimes(1)
    expect(healthyTask).toHaveBeenCalledTimes(1)

    rs.advanceTimersByTime(POLL_INTERVAL)
    await Promise.resolve()
    await Promise.resolve()

    expect(stuckTask).toHaveBeenCalledTimes(1)
    expect(healthyTask).toHaveBeenCalledTimes(2)
  })

  it('runs immediately and adopts a changed interval', async () => {
    const task = rs.fn(() => Promise.resolve())
    const customInterval = 1_000
    const [interval, setIntervalMs] = createSignal(POLL_INTERVAL)

    renderHook(() => usePolling([task], true, interval))
    flush()
    await Promise.resolve()
    expect(task).toHaveBeenCalledTimes(1)

    setIntervalMs(customInterval)
    flush()
    await Promise.resolve()
    expect(task).toHaveBeenCalledTimes(2)

    rs.advanceTimersByTime(customInterval)
    await Promise.resolve()
    expect(task).toHaveBeenCalledTimes(3)
  })
})
