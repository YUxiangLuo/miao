export const MAP_WIDTH = 1000
export const MAP_HEIGHT = 500

export function project(lng, lat, width = MAP_WIDTH, height = MAP_HEIGHT) {
  const x = ((Number(lng) + 180) / 360) * width
  const y = ((90 - Number(lat)) / 180) * height
  return { x, y }
}

export function unwrapLongitude(fromLng, toLng) {
  let lng = Number(toLng)
  const origin = Number(fromLng)
  const delta = lng - origin
  if (delta > 180) lng -= 360
  if (delta < -180) lng += 360
  return lng
}

export function curvePath(from, to) {
  const dx = to.x - from.x
  const dy = to.y - from.y
  const dist = Math.hypot(dx, dy) || 1
  const bulge = Math.min(72, Math.max(12, dist * 0.16))
  const mx = (from.x + to.x) / 2 - (dy / dist) * bulge
  const my = (from.y + to.y) / 2 + (dx / dist) * bulge
  return `M ${from.x.toFixed(1)} ${from.y.toFixed(1)} Q ${mx.toFixed(1)} ${my.toFixed(1)} ${to.x.toFixed(1)} ${to.y.toFixed(1)}`
}

function hasCoordinates(geo) {
  return geo && geo.latitude != null && geo.longitude != null
}

export function projectSegment(fromGeo, toGeo) {
  if (!hasCoordinates(fromGeo) || !hasCoordinates(toGeo)) return null

  const fromLng = Number(fromGeo.longitude)
  const toLng = Number(toGeo.longitude)
  const fromLat = Number(fromGeo.latitude)
  const toLat = Number(toGeo.latitude)
  const from = project(fromLng, fromLat)
  const to = project(toLng, toLat)

  if (Math.abs(toLng - fromLng) <= 180) {
    return { paths: [curvePath(from, to)] }
  }

  const unwrappedTo = unwrapLongitude(fromLng, toLng)
  const span = unwrappedTo - fromLng
  if (span === 0) {
    return { paths: [curvePath(from, to)] }
  }

  const edgeLng = span > 0 ? 180 : -180
  const edgeLat = fromLat + ((edgeLng - fromLng) / span) * (toLat - fromLat)
  return {
    paths: [
      curvePath(from, project(edgeLng, edgeLat)),
      curvePath(project(-edgeLng, edgeLat), to),
    ],
  }
}

export function flowPathData(clientGeo, destGeo, proxyGeo, isDirect) {
  if (!hasCoordinates(destGeo)) return []
  if (isDirect || !hasCoordinates(proxyGeo)) {
    return projectSegment(clientGeo, destGeo)?.paths || []
  }
  const first = projectSegment(clientGeo, proxyGeo)
  const second = projectSegment(proxyGeo, destGeo)
  return [...(first?.paths || []), ...(second?.paths || [])]
}

export function jitterGeo(geo, salt, amount = 1.15) {
  if (!geo || geo.latitude == null || geo.longitude == null) return null
  let hash = 2166136261
  const text = String(salt || '')
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  const angle = ((hash >>> 0) % 360) * (Math.PI / 180)
  const radius = (((hash >>> 8) % 100) / 100) * amount
  return {
    ...geo,
    latitude: geo.latitude + Math.sin(angle) * radius,
    longitude: geo.longitude + Math.cos(angle) * radius,
  }
}
