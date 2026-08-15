import { describe, expect, it } from 'vitest'
import { curvePath, flowPathData, project, projectSegment, unwrapLongitude } from './projection.js'

describe('map projection', () => {
  it('places the equator and prime meridian at the canvas center', () => {
    expect(project(0, 0, 1000, 500)).toEqual({ x: 500, y: 250 })
  })

  it('places the north-west corner at the origin', () => {
    expect(project(-180, 90, 1000, 500)).toEqual({ x: 0, y: 0 })
  })

  it('unwraps longitude so the short path crosses the antimeridian', () => {
    expect(unwrapLongitude(170, -170)).toBe(190)
    expect(unwrapLongitude(-170, 170)).toBe(-190)
  })

  it('builds a quadratic curve between two points', () => {
    const path = curvePath({ x: 0, y: 0 }, { x: 100, y: 0 })
    expect(path.startsWith('M 0.0 0.0 Q')).toBe(true)
    expect(path.endsWith('100.0 0.0')).toBe(true)
  })

  it('splits an antimeridian hop so the last point matches the marker', () => {
    const shanghai = { latitude: 31.2, longitude: 121.5 }
    const seattle = { latitude: 47.6, longitude: -122.3 }
    const segment = projectSegment(shanghai, seattle)
    const marker = project(-122.3, 47.6)

    expect(segment.paths).toHaveLength(2)
    expect(segment.paths[0].startsWith(`M ${project(121.5, 31.2).x.toFixed(1)}`)).toBe(true)
    expect(segment.paths[1].endsWith(`${marker.x.toFixed(1)} ${marker.y.toFixed(1)}`)).toBe(true)
  })

  it('still draws a proxy route when the exit has no coordinates', () => {
    const client = { latitude: 31.2, longitude: 121.5 }
    const dest = { latitude: 50.1, longitude: 8.6 }
    const paths = flowPathData(client, dest, null, false)
    expect(paths).toHaveLength(1)
    expect(paths[0].endsWith(`${project(8.6, 50.1).x.toFixed(1)} ${project(8.6, 50.1).y.toFixed(1)}`)).toBe(true)
  })
})
