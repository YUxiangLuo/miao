// W3C DTCG 2025.10 aligned canonical motion tokens — contract-locked to src/styles/theme.css.

export const MOTION_PREFERENCES = ['system', 'full', 'reduced', 'off'] as const
export type MotionPreference = (typeof MOTION_PREFERENCES)[number]

export const MOTION_PREFERENCE_STORAGE_KEY = 'harbor-motion'

export const MOTION_DURATIONS_MS = {
  instant: 0,
  micro: 80,
  fast: 140,
  normal: 200,
  emphasis: 260,
} as const

export type MotionDurationToken = keyof typeof MOTION_DURATIONS_MS

export const MOTION_REDUCED_FADE_MS = 90
export const MOTION_REDUCED_SPIN_MS = 1800

export const MOTION_TELEMETRY_MS = {
  indeterminateSweep: 1400,
  statusBreath: 2000,
  statusBreathReduced: 2400,
  shimmerSweep: 1800,
} as const

export const MOTION_FLASH_TIMING_MS = {
  readbackAttack: 60,
  readbackHold: 360,
  readbackDecay: 840,
  postMoveDecay: 680,
  deltaAttack: 60,
  deltaHold: 140,
  deltaDecay: 420,
} as const

export const MOTION_DISTANCES_PX = {
  xs: 4,
  sm: 8,
  md: 12,
  lg: 20,
} as const

export type MotionDistanceToken = keyof typeof MOTION_DISTANCES_PX

export const MOTION_EASINGS = {
  productive: {
    css: 'cubic-bezier(0.2, 0, 0.38, 0.9)',
    dtcg: [0.2, 0, 0.38, 0.9] as const,
  },
  enter: {
    css: 'cubic-bezier(0.23, 1, 0.32, 1)',
    dtcg: [0.23, 1, 0.32, 1] as const,
  },
  exit: {
    css: 'cubic-bezier(0.2, 0, 0.2, 1)',
    dtcg: [0.2, 0, 0.2, 1] as const,
  },
  spatial: {
    // M3 Standard Fast Spatial (stiffness=1400, zeta=0.9, 0.15% near-zero overshoot).
    css: 'linear(0, 0.0045 1.3%, 0.0205 2.8%, 0.0506 4.5%, 0.09 6.3%, 0.1789 9.5%, 0.3928 16.5%, 0.4873 19.8%, 0.5798 23.3%, 0.6611 26.8%, 0.7262 30%, 0.7854 33.5%, 0.8342 37%, 0.8763 40.8%, 0.9131 45%, 0.9433 49.8%, 0.9661 55%, 0.9826 61%, 0.9986 75%, 1)',
    dtcg: [0.2, 0, 0, 1] as const,
  },
  linear: {
    css: 'linear',
    dtcg: [0, 0, 1, 1] as const,
  },
} as const

export type MotionEasingToken = keyof typeof MOTION_EASINGS

export const MOTION_ARCHETYPES = [
  'scrim',
  'elevate',
  'slide-edge',
  'float',
  'stack-item',
  'reveal',
  'tactile',
  'indicator',
  'pulse-status',
] as const

export type MotionArchetype = (typeof MOTION_ARCHETYPES)[number]

export const MOTION_PROFILES = ['full', 'reduced', 'off'] as const
export type MotionProfile = (typeof MOTION_PROFILES)[number]
