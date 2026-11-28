import { createEffect, createMemo, createSignal } from 'solid-js'
import {
  getEffectiveMotionScale,
  getResolvedMotionProfile,
  resolveScaledDurationMs,
  supportsWebAnimations,
} from './policy'
import type { MotionDurationToken } from './tokens'

export type PresencePhase = 'unmounted' | 'open' | 'exiting'

export interface UsePresenceOptions {
  open: () => boolean
  /** Must point to the longest-exiting animated node (e.g. Dialog/Sheet panel 140ms, NOT scrim 80ms). */
  element: () => HTMLElement | null
  exitDurationToken?: MotionDurationToken
  onExitStart?: () => void
}

export interface PresenceState {
  mounted: () => boolean
  phase: () => PresencePhase
  motionState: () => 'open' | 'closed'
}

/**
 * Component-agnostic SolidJS 2.0 presence primitive for interruptible enter/exit lifecycles.
 * Synchronously unmounts when Web Animations API is unavailable (jsdom), profile is "off", or scale is 0.
 */
export function usePresence(options: UsePresenceOptions): PresenceState {
  const isOpen = createMemo(() => Boolean(options.open()))
  const canAnimateExit = createMemo(
    () => supportsWebAnimations() && getResolvedMotionProfile() !== 'off' && getEffectiveMotionScale() > 0,
  )

  const [mounted, setMounted] = createSignal<boolean>((prev) => (
    isOpen() ? true : canAnimateExit() ? Boolean(prev) : false
  ))

  const phase = createMemo<PresencePhase>(() => {
    if (isOpen()) return 'open'
    return mounted() ? 'exiting' : 'unmounted'
  })

  const motionState = createMemo<'open' | 'closed'>(() => (isOpen() ? 'open' : 'closed'))

  const exitTarget = createMemo(
    () => {
      if (isOpen() || !mounted() || !canAnimateExit()) return null
      return {
        el: options.element(),
        maxWaitMs: resolveScaledDurationMs(options.exitDurationToken ?? 'fast') + 80,
      }
    },
    { equals: (a, b) => a?.el === b?.el && a?.maxWaitMs === b?.maxWaitMs },
  )

  createEffect(exitTarget, (target) => {
    if (!target) return undefined
    options.onExitStart?.()

    const { el, maxWaitMs } = target
    let cancelled = false
    const finish = () => {
      if (!cancelled) {
        setMounted(false)
      }
    }

    if (!el || typeof el.getAnimations !== 'function') {
      queueMicrotask(finish)
      return () => {
        cancelled = true
      }
    }

    const fallbackTimer = window.setTimeout(finish, maxWaitMs)

    queueMicrotask(() => {
      if (cancelled) return
      if (typeof el.getAnimations !== 'function') {
        window.clearTimeout(fallbackTimer)
        finish()
        return
      }
      const active = el.getAnimations({ subtree: false })
      if (active.length === 0) {
        window.clearTimeout(fallbackTimer)
        finish()
        return
      }
      void Promise.allSettled(active.map((anim) => anim.finished)).then(() => {
        window.clearTimeout(fallbackTimer)
        finish()
      })
    })

    return () => {
      cancelled = true
      window.clearTimeout(fallbackTimer)
    }
  })

  return { mounted, phase, motionState }
}
