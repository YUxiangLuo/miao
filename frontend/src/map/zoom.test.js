import { describe, expect, it } from 'vitest'
import {
  BASE_VIEW_BOX,
  MAX_SCALE,
  clampViewBox,
  initialViewBox,
  panBy,
  scaleOf,
  zoomAtPoint,
} from './zoom.js'

describe('zoom', () => {
  it('starts at the base view box with scale 1', () => {
    expect(initialViewBox()).toEqual(BASE_VIEW_BOX)
    expect(scaleOf(initialViewBox())).toBe(1)
  })

  it('zooms in around the anchor point, which stays fixed', () => {
    const vb = initialViewBox()
    const cx = 500
    const cy = 250
    const zoomed = zoomAtPoint(vb, cx, cy, 2)

    expect(zoomed.w).toBeCloseTo(vb.w / 2)
    expect(scaleOf(zoomed)).toBeCloseTo(2)
    // 锚点在视图中的相对位置不变
    expect((cx - zoomed.x) / zoomed.w).toBeCloseTo((cx - vb.x) / vb.w)
    expect((cy - zoomed.y) / zoomed.h).toBeCloseTo((cy - vb.y) / vb.h)
  })

  it('clamps at max scale', () => {
    const zoomed = zoomAtPoint(initialViewBox(), 500, 250, 100)
    expect(scaleOf(zoomed)).toBe(MAX_SCALE)
  })

  it('zooming back out past 1x returns to the base box', () => {
    const zoomed = zoomAtPoint(initialViewBox(), 100, 100, 0.5)
    expect(zoomed).toEqual(BASE_VIEW_BOX)
  })

  it('keeps the view inside the map when zooming near an edge', () => {
    const zoomed = zoomAtPoint(initialViewBox(), 0, 30, 4)
    expect(zoomed.x).toBeGreaterThanOrEqual(BASE_VIEW_BOX.x)
    expect(zoomed.y).toBeGreaterThanOrEqual(BASE_VIEW_BOX.y)
  })

  it('clamps panning to the map bounds', () => {
    const zoomed = zoomAtPoint(initialViewBox(), 500, 250, 4)
    const panned = panBy(zoomed, -9999, 9999)
    expect(panned.x).toBe(BASE_VIEW_BOX.x)
    expect(panned.y + panned.h).toBe(BASE_VIEW_BOX.y + BASE_VIEW_BOX.h)
  })

  it('keeps the locked aspect ratio when clamping', () => {
    const clamped = clampViewBox({ x: 0, y: 30, w: 250, h: 999 })
    expect(clamped.h).toBeCloseTo(250 * (BASE_VIEW_BOX.h / BASE_VIEW_BOX.w))
  })
})
