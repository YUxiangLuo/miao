import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { Show, createSignal, flush } from 'solid-js'
import { render, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, rs } from '@rstest/core'
import {
  MOTION_ARCHETYPES,
  MOTION_DISTANCES_PX,
  MOTION_DURATIONS_MS,
  MOTION_EASINGS,
  MOTION_REDUCED_FADE_MS,
  flashSurface,
  flipElements,
  getMotionPreference,
  getResolvedMotionProfile,
  getScrollBehavior,
  initMotionPolicy,
  peekReducedMotion,
   peekScaledDurationMs,
  resetMotionPolicyForTest,
  scrollIntoViewSafe,
  setMotionPreference,
  setMotionScaleOverride,
  usePresence,
} from './index'

const THEME_CSS_PATH = resolve(process.cwd(), 'src/styles/theme.css')

afterEach(() => {
  rs.unstubAllGlobals()
  resetMotionPolicyForTest()
})

describe('Motion token parity with theme.css', () => {
  it('locks canonical durations, distances, easings, and all 9 archetypes to CSS variables & selectors', () => {
    const css = readFileSync(THEME_CSS_PATH, 'utf8')

    for (const [token, ms] of Object.entries(MOTION_DURATIONS_MS)) {
      expect(css).toContain(`--duration-base-${token}: ${ms}ms;`)
    }
    expect(css).toContain(`--duration-reduced-fade: ${MOTION_REDUCED_FADE_MS}ms;`)

    for (const [token, px] of Object.entries(MOTION_DISTANCES_PX)) {
      expect(css).toContain(`--motion-distance-base-${token}: ${px}px;`)
    }

    for (const [token, def] of Object.entries(MOTION_EASINGS)) {
      expect(css).toContain(`--motion-ease-${token}: ${def.css};`)
    }

    for (const archetype of MOTION_ARCHETYPES) {
      expect(css).toContain(`[data-motion="${archetype}"]`)
    }
  })
})

describe('Motion policy state machine & 3-tier degradation', () => {
  it('resolves system/full/reduced/off preferences, OS media changes, and scale overrides', () => {
    let osMatches = false
    const listeners: Array<(event: MediaQueryListEvent) => void> = []

    rs.stubGlobal('matchMedia', rs.fn().mockImplementation(() => ({
      get matches() {
        return osMatches
      },
      addEventListener: (_type: string, cb: (event: MediaQueryListEvent) => void) => {
        listeners.push(cb)
      },
      removeEventListener: rs.fn(),
    })))
    window.matchMedia = globalThis.matchMedia

    initMotionPolicy()
    flush()
    expect(getMotionPreference()).toBe('system')
    expect(getResolvedMotionProfile()).toBe('full')
    expect(document.documentElement.hasAttribute('data-motion-profile')).toBe(false)
    expect(peekScaledDurationMs('normal')).toBe(200)
    expect(getScrollBehavior()).toBe('smooth')

    // OS prefers-reduced-motion flips to true while on 'system'
    osMatches = true
    for (const cb of listeners) cb({ matches: true } as MediaQueryListEvent)
    flush()
    expect(getResolvedMotionProfile()).toBe('reduced')
    expect(document.documentElement.dataset.motionProfile).toBe('reduced')
    expect(peekScaledDurationMs('normal')).toBe(MOTION_REDUCED_FADE_MS)
    expect(peekReducedMotion()).toBe(true)
    expect(getScrollBehavior()).toBe('auto')

    // Explicit 'full' overrides OS reduced motion
    setMotionPreference('full')
    flush()
    expect(getResolvedMotionProfile()).toBe('full')
    expect(document.documentElement.dataset.motionProfile).toBe('full')
    expect(peekReducedMotion()).toBe(false)

    // Explicit 'off' collapses all durations to 0ms
    setMotionPreference('off')
    flush()
    expect(getResolvedMotionProfile()).toBe('off')
    expect(document.documentElement.dataset.motionProfile).toBe('off')
    expect(peekScaledDurationMs('emphasis')).toBe(0)

    // Runtime scale override scales durations proportionally
    setMotionPreference('full')
    setMotionScaleOverride(0.5)
    flush()
    expect(peekScaledDurationMs('normal')).toBe(100)
    expect(document.documentElement.style.getPropertyValue('--motion-scale')).toBe('0.5')
  })
})

describe('usePresence lifecycle & imperative WAAPI primitives', () => {
  it('unmounts synchronously when Web Animations API getAnimations is absent (jsdom default)', () => {
    const [open, setOpen] = createSignal(true)
    const [el, setEl] = createSignal<HTMLElement | null>(null)

    function Probe() {
      const presence = usePresence({ open, element: el })
      return (
        <Show when={presence.mounted()}>
          <div ref={setEl} data-testid="box" data-state={presence.motionState()} />
        </Show>
      )
    }

    const { queryByTestId } = render(() => <Probe />)
    expect(queryByTestId('box')).toHaveAttribute('data-state', 'open')

    setOpen(false)
    flush()
    expect(queryByTestId('box')).toBeNull()
  })

  it('waits for active WAAPI animation finished promise before unmounting in real browsers', async () => {
    let finishAnim!: () => void
    const finished = new Promise<Animation>((resolve) => {
      finishAnim = () => resolve({} as Animation)
    })

    const origGetAnimations = Element.prototype.getAnimations
    Element.prototype.getAnimations = () => [{ finished } as unknown as Animation]

    try {
      setMotionPreference('full')
      flush()
      const [open, setOpen] = createSignal(true)
      const [el, setEl] = createSignal<HTMLElement | null>(null)

      function Probe() {
        const presence = usePresence({ open, element: el, exitDurationToken: 'fast' })
        return (
          <Show when={presence.mounted()}>
            <div ref={setEl} data-testid="box" data-state={presence.motionState()} />
          </Show>
        )
      }

      const { queryByTestId } = render(() => <Probe />)
      expect(queryByTestId('box')).toHaveAttribute('data-state', 'open')

      setOpen(false)
      flush()
      // Remains mounted in closed exiting state while WAAPI transition runs
      expect(queryByTestId('box')).toHaveAttribute('data-state', 'closed')

      finishAnim()
      await waitFor(() => {
        flush()
        expect(queryByTestId('box')).toBeNull()
      })
    } finally {
      if (origGetAnimations) {
        Element.prototype.getAnimations = origGetAnimations
      } else {
        Reflect.deleteProperty(Element.prototype, 'getAnimations')
      }
    }
  })

  it('runs flashSurface, bounded flipElements, and scrollIntoViewSafe with motion degradation', () => {
    setMotionPreference('full')
    const el = document.createElement('div')
    document.body.appendChild(el)

    const animateSpy = rs.fn().mockReturnValue({
      cancel: rs.fn(),
      finished: Promise.resolve(),
    })
    el.animate = animateSpy
    const scrollSpy = rs.fn()
    el.scrollIntoView = scrollSpy

    flashSurface(el, 'active')
    expect(animateSpy).toHaveBeenCalledTimes(1)

    let rectLeft = 0
    el.getBoundingClientRect = () => ({ left: rectLeft, top: 0, width: 100, height: 40 } as DOMRect)
    flipElements([el], () => {
      rectLeft = 120
    })
    expect(animateSpy).toHaveBeenCalledTimes(2)

    scrollIntoViewSafe(el, { block: 'nearest' })
    expect(scrollSpy).toHaveBeenLastCalledWith({ block: 'nearest', behavior: 'smooth' })

    // Under reduced motion, FLIP skips transform animation and scrollIntoView degrades to auto
    setMotionPreference('reduced')
    flipElements([el], () => {
      rectLeft = 240
    })
    expect(animateSpy).toHaveBeenCalledTimes(2)

    scrollIntoViewSafe(el, { block: 'nearest' })
    expect(scrollSpy).toHaveBeenLastCalledWith({ block: 'nearest', behavior: 'auto' })

    el.remove()
  })
})
