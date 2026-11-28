import {
  getScrollBehavior,
  peekMotionProfile,
  peekMotionScale,
  peekReducedMotion,
  peekScaledDurationMs,
} from './policy'
import { MOTION_EASINGS, MOTION_FLASH_TIMING_MS, type MotionDurationToken } from './tokens'

export type FlashTone = 'success' | 'active' | 'warning' | 'destructive'

export interface FlashSurfaceOptions {
  attackMs?: number
  holdMs?: number
  decayMs?: number
  baseBg?: string
}

const FLASH_COLOR_VARS: Record<FlashTone, string> = {
  success: 'var(--color-success-soft)',
  active: 'var(--color-surface-active)',
  warning: 'var(--color-warning-soft)',
  destructive: 'var(--color-destructive-soft)',
}

const activeFlashAnimations = new WeakMap<HTMLElement, Animation>()
const activeFlipAnimations = new WeakMap<HTMLElement, Animation>()
const activeIndicatorAnimations = new WeakMap<HTMLElement, Animation>()

function canRunWaapi(el: HTMLElement | null | undefined): el is HTMLElement {
  return Boolean(el && typeof el.animate === 'function')
}

function isFrozenSubtree(el: HTMLElement): boolean {
  return Boolean(el.closest('[data-motion-freeze="true"]'))
}

/**
 * Triggers an asymmetric confirmation highlight flash (fast attack -> short hold -> gentle decay).
 * Safe no-op in jsdom or frozen/off states.
 */
export function flashSurface(
  el: HTMLElement | null | undefined,
  tone: FlashTone,
  options: FlashSurfaceOptions = {},
): void {
  if (!canRunWaapi(el) || isFrozenSubtree(el)) return
  const profile = peekMotionProfile()
  const scale = peekMotionScale()
  if (profile === 'off' || scale <= 0) return

  activeFlashAnimations.get(el)?.cancel()

  const attackMs = Math.round((options.attackMs ?? MOTION_FLASH_TIMING_MS.readbackAttack) * scale)
  const holdMs = Math.round((options.holdMs ?? (tone === 'active' ? 180 : MOTION_FLASH_TIMING_MS.readbackHold)) * scale)
  const decayMs = Math.round((options.decayMs ?? MOTION_FLASH_TIMING_MS.readbackDecay) * scale)
  const totalMs = Math.max(1, attackMs + holdMs + decayMs)

  const highlight = FLASH_COLOR_VARS[tone]
  const baseBg = options.baseBg !== undefined
    ? options.baseBg
    : typeof window !== 'undefined' && typeof window.getComputedStyle === 'function'
      ? window.getComputedStyle(el).backgroundColor
      : 'transparent'

  const keyframes: Keyframe[] = profile === 'reduced'
    ? [
        { backgroundColor: highlight, offset: 0 },
        { backgroundColor: highlight, offset: 0.35 },
        { backgroundColor: baseBg, offset: 1 },
      ]
    : [
        { backgroundColor: baseBg, offset: 0 },
        { backgroundColor: highlight, offset: attackMs / totalMs },
        { backgroundColor: highlight, offset: (attackMs + holdMs) / totalMs },
        { backgroundColor: baseBg, offset: 1 },
      ]

  const animation = el.animate(keyframes, {
    duration: totalMs,
    easing: MOTION_EASINGS.productive.css,
  })
  activeFlashAnimations.set(el, animation)
  void animation.finished
    ?.catch(() => undefined)
    .finally(() => {
      if (activeFlashAnimations.get(el) === animation) {
        activeFlashAnimations.delete(el)
      }
    })
}

export interface FlipElementsOptions {
  durationToken?: MotionDurationToken
  maxElements?: number
}

/**
 * Performs GPU-composited FLIP animations for bounded collections (e.g. node grid sort, toast reflow).
 * Automatically skips when reduced-motion is active, scale is 0, or element count exceeds maxElements.
 */
export function flipElements(
  elements: readonly HTMLElement[],
  mutate: () => void,
  options: FlipElementsOptions = {},
): void {
  const maxElements = options.maxElements ?? 32
  if (elements.length === 0 || elements.length > maxElements || peekReducedMotion()) {
    mutate()
    return
  }

  const beforeRects = new Map<HTMLElement, DOMRect>()
  for (const el of elements) {
    if (!canRunWaapi(el) || isFrozenSubtree(el)) continue
    beforeRects.set(el, el.getBoundingClientRect())
  }

  mutate()

  const durationMs = peekScaledDurationMs(options.durationToken ?? 'normal')
  if (durationMs <= 0) return

  for (const [el, before] of beforeRects) {
    if (!el.isConnected) continue
    const after = el.getBoundingClientRect()
    const dx = before.left - after.left
    const dy = before.top - after.top
    if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) continue

    activeFlipAnimations.get(el)?.cancel()
    const animation = el.animate(
      [
        { transform: `translate3d(${dx}px, ${dy}px, 0)` },
        { transform: 'translate3d(0, 0, 0)' },
      ],
      {
        duration: durationMs,
        easing: MOTION_EASINGS.spatial.css,
      },
    )
    activeFlipAnimations.set(el, animation)
    void animation.finished
      ?.catch(() => undefined)
      .finally(() => {
        if (activeFlipAnimations.get(el) === animation) {
          activeFlipAnimations.delete(el)
        }
      })
  }
}

/** Smoothly translates and scales a shared tab/segmented indicator between two rects using GPU transforms. */
export function animateIndicatorMove(
  indicatorEl: HTMLElement | null | undefined,
  fromRect: DOMRect,
  toRect: DOMRect,
): void {
  if (!canRunWaapi(indicatorEl) || isFrozenSubtree(indicatorEl) || peekReducedMotion()) return
  if (toRect.width <= 0 || fromRect.width <= 0) return

  const dx = fromRect.left - toRect.left
  const scaleX = fromRect.width / toRect.width
  if (Math.abs(dx) < 0.5 && Math.abs(scaleX - 1) < 0.01) return

  const durationMs = peekScaledDurationMs('normal')
  if (durationMs <= 0) return

  activeIndicatorAnimations.get(indicatorEl)?.cancel()
  const animation = indicatorEl.animate(
    [
      { transform: `translate3d(${dx}px, 0, 0) scaleX(${scaleX})` },
      { transform: 'translate3d(0, 0, 0) scaleX(1)' },
    ],
    {
      duration: durationMs,
      easing: MOTION_EASINGS.spatial.css,
    },
  )
  activeIndicatorAnimations.set(indicatorEl, animation)
  void animation.finished
    ?.catch(() => undefined)
    .finally(() => {
      if (activeIndicatorAnimations.get(indicatorEl) === animation) {
        activeIndicatorAnimations.delete(indicatorEl)
      }
    })
}

/** Motion-policy-aware scrollIntoView wrapper that degrades smooth scrolling to instant under reduced/off profiles. */
export function scrollIntoViewSafe(
  el: Element | null | undefined,
  options: Omit<ScrollIntoViewOptions, 'behavior'> = {},
): void {
  if (!el || typeof el.scrollIntoView !== 'function') return
  el.scrollIntoView({
    ...options,
    behavior: getScrollBehavior(),
  })
}
