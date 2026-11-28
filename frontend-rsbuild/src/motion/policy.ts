import { createMemo, createRoot, createSignal, runWithOwner } from 'solid-js'
import {
  MOTION_DURATIONS_MS,
  MOTION_PREFERENCES,
  MOTION_PREFERENCE_STORAGE_KEY,
  MOTION_REDUCED_FADE_MS,
  type MotionDurationToken,
  type MotionPreference,
  type MotionProfile,
} from './tokens'

interface MotionPolicySignals {
  preference: () => MotionPreference
  setPreference: (next: MotionPreference) => void
  osReduced: () => boolean
  setOsReduced: (next: boolean) => void
  scaleOverride: () => number | null
  setScaleOverride: (next: number | null) => void
  resolvedProfile: () => MotionProfile
  effectiveScale: () => number
  dispose: () => void
}

const REDUCED_MOTION_QUERY = '(prefers-reduced-motion: reduce)'

function isMotionPreference(value: string | null): value is MotionPreference {
  return value !== null && (MOTION_PREFERENCES as readonly string[]).includes(value)
}

function readStoredPreference(): MotionPreference {
  if (typeof window === 'undefined') return 'system'
  try {
    const raw = window.localStorage.getItem(MOTION_PREFERENCE_STORAGE_KEY)
    return isMotionPreference(raw) ? raw : 'system'
  } catch {
    return 'system'
  }
}

function writeStoredPreference(preference: MotionPreference): void {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(MOTION_PREFERENCE_STORAGE_KEY, preference)
  } catch {
    /* Ignore storage errors in private/restricted contexts */
  }
}

function readOsReducedMotion(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false
  return Boolean(window.matchMedia(REDUCED_MOTION_QUERY)?.matches)
}

// Plain module mirrors allow imperative callers (WAAPI primitives, event handlers)
// to read motion state without triggering Solid 2.0 STRICT_READ_UNTRACKED diagnostics.
let preferenceMirror: MotionPreference = readStoredPreference()
let osReducedMirror = readOsReducedMotion()
let scaleOverrideMirror: number | null = null
let policySignals: MotionPolicySignals | null = null
let mediaQueryList: MediaQueryList | null = null
let mediaQueryListener: ((event: MediaQueryListEvent) => void) | null = null

function computeResolvedProfile(preference: MotionPreference, osReduced: boolean): MotionProfile {
  if (preference === 'off') return 'off'
  if (preference === 'reduced') return 'reduced'
  if (preference === 'full') return 'full'
  return osReduced ? 'reduced' : 'full'
}

function readDomMotionScale(): number {
  if (typeof document === 'undefined' || typeof window === 'undefined') return 1
  const root = document.documentElement
  const inlineRaw = root.style.getPropertyValue('--motion-scale').trim()
  const raw = inlineRaw || (
    typeof window.getComputedStyle === 'function'
      ? window.getComputedStyle(root).getPropertyValue('--motion-scale').trim()
      : ''
  )
  if (!raw) return 1
  const parsed = Number.parseFloat(raw)
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 1
}

function computeEffectiveScale(profile: MotionProfile, override: number | null): number {
  if (profile === 'off') return 0
  if (override !== null) return override
  return readDomMotionScale()
}

function syncRootAttributes(
  profile: MotionProfile,
  preference: MotionPreference,
  scaleOverride: number | null,
): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  if (preference === 'system' && profile === 'full') {
    root.removeAttribute('data-motion-profile')
  } else {
    root.dataset.motionProfile = profile
  }

  if (scaleOverride !== null) {
    root.style.setProperty('--motion-scale', String(scaleOverride))
  } else {
    root.style.removeProperty('--motion-scale')
  }
}

function getPolicySignals(): MotionPolicySignals {
  if (!policySignals) {
    preferenceMirror = readStoredPreference()
    osReducedMirror = readOsReducedMotion()
    policySignals = runWithOwner(null, () =>
      createRoot((dispose) => {
        const [preference, setPreference] = createSignal<MotionPreference>(preferenceMirror)
        const [osReduced, setOsReduced] = createSignal<boolean>(osReducedMirror)
        const [scaleOverride, setScaleOverride] = createSignal<number | null>(scaleOverrideMirror)

        const resolvedProfile = createMemo<MotionProfile>(() => computeResolvedProfile(preference(), osReduced()))
        const effectiveScale = createMemo<number>(() => computeEffectiveScale(resolvedProfile(), scaleOverride()))

        return {
          preference,
          setPreference,
          osReduced,
          setOsReduced,
          scaleOverride,
          setScaleOverride,
          resolvedProfile,
          effectiveScale,
          dispose,
        }
      }),
    )
  }
  return policySignals
}

function resolveDurationForProfile(token: MotionDurationToken, profile: MotionProfile, scale: number): number {
  if (profile === 'off' || scale <= 0 || token === 'instant') return 0
  if (profile === 'reduced' && (token === 'fast' || token === 'normal' || token === 'emphasis')) {
    return Math.round(MOTION_REDUCED_FADE_MS * scale)
  }
  return Math.round(MOTION_DURATIONS_MS[token] * scale)
}

/** Initializes OS reduced-motion subscription and synchronizes root dataset attributes. */
export function initMotionPolicy(): void {
  const signals = getPolicySignals()
  preferenceMirror = readStoredPreference()
  osReducedMirror = readOsReducedMotion()
  signals.setPreference(preferenceMirror)
  signals.setOsReduced(osReducedMirror)

  if (typeof window !== 'undefined' && typeof window.matchMedia === 'function' && !mediaQueryList) {
    const mql = window.matchMedia(REDUCED_MOTION_QUERY)
    const onChange = (event: MediaQueryListEvent) => {
      osReducedMirror = event.matches
      policySignals?.setOsReduced(event.matches)
      syncRootAttributes(
        computeResolvedProfile(preferenceMirror, osReducedMirror),
        preferenceMirror,
        scaleOverrideMirror,
      )
    }
    if (typeof mql?.addEventListener === 'function') {
      mql.addEventListener('change', onChange)
      mediaQueryList = mql
      mediaQueryListener = onChange
    }
  }

  syncRootAttributes(
    computeResolvedProfile(preferenceMirror, osReducedMirror),
    preferenceMirror,
    scaleOverrideMirror,
  )
}

/** Updates and persists explicit user motion preference ("system" | "full" | "reduced" | "off"). */
export function setMotionPreference(preference: MotionPreference): void {
  const unchanged = preferenceMirror === preference
  preferenceMirror = preference
  writeStoredPreference(preference)
  osReducedMirror = readOsReducedMotion()
  if (policySignals) {
    if (!unchanged) policySignals.setPreference(preference)
    policySignals.setOsReduced(osReducedMirror)
  }
  syncRootAttributes(
    computeResolvedProfile(preferenceMirror, osReducedMirror),
    preferenceMirror,
    scaleOverrideMirror,
  )
}

/** Sets or clears a runtime speed multiplier override (e.g. 0.5 for faster, 0 to freeze, null to clear). */
export function setMotionScaleOverride(scale: number | null): void {
  const normalized = scale === null ? null : Math.max(0, Number.isFinite(scale) ? scale : 1)
  const unchanged = scaleOverrideMirror === normalized
  scaleOverrideMirror = normalized
  if (policySignals && !unchanged) {
    policySignals.setScaleOverride(normalized)
  }
  syncRootAttributes(
    computeResolvedProfile(preferenceMirror, osReducedMirror),
    preferenceMirror,
    scaleOverrideMirror,
  )
}

/** Detects real browser Web Animations API getAnimations support (false in jsdom so presence unmounts synchronously). */
export function supportsWebAnimations(): boolean {
  return typeof Element !== 'undefined' && typeof Element.prototype.getAnimations === 'function'
}

/** Reactive accessor for user motion preference — use inside memos/effects/JSX only. */
export function getMotionPreference(): MotionPreference {
  return getPolicySignals().preference()
}

/** Reactive accessor for resolved profile ("full" | "reduced" | "off") — use inside memos/effects/JSX only. */
export function getResolvedMotionProfile(): MotionProfile {
  return getPolicySignals().resolvedProfile()
}

/** Reactive accessor for effective motion scale — use inside memos/effects/JSX only. */
export function getEffectiveMotionScale(): number {
  return getPolicySignals().effectiveScale()
}

/** Reactive duration resolver — use inside memos/effects/JSX only. */
export function resolveScaledDurationMs(token: MotionDurationToken): number {
  const signals = getPolicySignals()
  return resolveDurationForProfile(token, signals.resolvedProfile(), signals.effectiveScale())
}

/** Non-tracking snapshot for imperative callers (event handlers, timers, WAAPI helpers). */
export function peekMotionProfile(): MotionProfile {
  const liveOsReduced = mediaQueryList ? osReducedMirror : readOsReducedMotion()
  return computeResolvedProfile(preferenceMirror, liveOsReduced)
}

/** Non-tracking scale snapshot for imperative callers. */
export function peekMotionScale(): number {
  return computeEffectiveScale(peekMotionProfile(), scaleOverrideMirror)
}

/** Non-tracking scaled duration snapshot for imperative callers. */
export function peekScaledDurationMs(token: MotionDurationToken): number {
  return resolveDurationForProfile(token, peekMotionProfile(), peekMotionScale())
}

/** Non-tracking reduced-motion predicate for imperative callers. */
export function peekReducedMotion(): boolean {
  return peekMotionProfile() !== 'full' || peekMotionScale() <= 0
}

/** Non-tracking scroll behavior helper respecting reduced motion and off profiles. */
export function getScrollBehavior(): ScrollBehavior {
  return peekReducedMotion() ? 'auto' : 'smooth'
}

/** Test-only helper to completely reset singleton state, OS listener, and root DOM attributes. */
export function resetMotionPolicyForTest(): void {
  if (mediaQueryList && mediaQueryListener && typeof mediaQueryList.removeEventListener === 'function') {
    mediaQueryList.removeEventListener('change', mediaQueryListener)
  }
  mediaQueryList = null
  mediaQueryListener = null

  policySignals?.dispose()
  policySignals = null

  preferenceMirror = 'system'
  osReducedMirror = false
  scaleOverrideMirror = null

  if (typeof window !== 'undefined') {
    try {
      window.localStorage.removeItem(MOTION_PREFERENCE_STORAGE_KEY)
    } catch {
      /* ignore */
    }
  }

  if (typeof document !== 'undefined') {
    document.documentElement.removeAttribute('data-motion-profile')
    document.documentElement.style.removeProperty('--motion-scale')
  }
}
