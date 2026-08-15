import { describe, expect, it } from 'vitest'
import {
  MAX_PARTICLES_TOTAL,
  MAX_FLOW_WIDTH,
  MIN_FLOW_WIDTH,
  STARFIELD,
  allocateParticles,
  buildStarfield,
  flowWidth,
  particleCount,
  particleDuration,
} from './visuals.js'

describe('flowWidth', () => {
  it('maps zero speed to the minimum width', () => {
    expect(flowWidth(0)).toBe(MIN_FLOW_WIDTH)
    expect(flowWidth(undefined)).toBe(MIN_FLOW_WIDTH)
  })

  it('saturates at the maximum width for very fast flows', () => {
    expect(flowWidth(10 * 1024 * 1024)).toBe(MAX_FLOW_WIDTH)
  })

  it('grows monotonically with speed', () => {
    const widths = [0, 1024, 10 * 1024, 100 * 1024, 1024 * 1024].map(flowWidth)
    for (let index = 1; index < widths.length; index += 1) {
      expect(widths[index]).toBeGreaterThanOrEqual(widths[index - 1])
    }
  })
})

describe('particleCount', () => {
  it('returns 0 below the active threshold', () => {
    expect(particleCount(0)).toBe(0)
    expect(particleCount(1023)).toBe(0)
  })

  it('scales up to the per-path cap', () => {
    expect(particleCount(1024)).toBe(1)
    expect(particleCount(10 * 1024)).toBe(2)
    expect(particleCount(100 * 1024)).toBe(3)
    expect(particleCount(50 * 1024 * 1024)).toBe(3)
  })
})

describe('particleDuration', () => {
  it('slow flows orbit slowly, fast flows quickly', () => {
    expect(particleDuration(1024)).toBeCloseTo(4)
    expect(particleDuration(1024 * 1024 * 1024)).toBeCloseTo(1.2)
  })
})

describe('starfield', () => {
  it('is deterministic and bounded', () => {
    const a = buildStarfield(10, 1000, 500)
    const b = buildStarfield(10, 1000, 500)
    expect(a).toEqual(b)
    for (const star of a) {
      expect(star.x).toBeGreaterThanOrEqual(0)
      expect(star.x).toBeLessThan(1000)
      expect(star.y).toBeGreaterThanOrEqual(0)
      expect(star.y).toBeLessThan(500)
    }
  })

  it('exports a non-empty module-level starfield', () => {
    expect(STARFIELD.length).toBeGreaterThan(0)
  })
})

describe('allocateParticles', () => {
  it('respects the total budget and prefers faster paths', () => {
    const paths = Array.from({ length: 40 }, (_, index) => ({
      id: `p${index}`,
      speed: (index + 1) * 100 * 1024,
    }))
    const allocation = allocateParticles(paths)
    const total = [...allocation.values()].reduce((sum, count) => sum + count, 0)
    expect(total).toBeLessThanOrEqual(MAX_PARTICLES_TOTAL)
    // 最快的一定拿到配额
    expect(allocation.get('p39')).toBe(3)
    // 50 个预算 / 每条最多 3 个 → 至少有路径被挤出
    expect(allocation.size).toBeLessThan(40)
  })

  it('skips inactive paths', () => {
    const allocation = allocateParticles([{ id: 'a', speed: 100 }])
    expect(allocation.size).toBe(0)
  })
})
