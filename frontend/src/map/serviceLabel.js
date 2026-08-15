import { iconForDomain, normalizeDomain } from '../components/siteIcons.js'

function isIpAddress(value) {
  if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(value)) return true
  return value.includes(':')
}

export function serviceForDestination(domain, ip) {
  const host = domain || ip || 'unknown'
  const icon = iconForDomain(host)
  if (icon.id !== 'letter') {
    return { id: icon.id, label: icon.label, icon }
  }

  const normalized = normalizeDomain(host)
  if (!normalized || normalized === 'unknown') {
    return { id: `raw:${host}`, label: host || 'unknown', icon }
  }
  if (isIpAddress(normalized)) {
    return { id: `ip:${normalized}`, label: normalized, icon }
  }

  const parts = normalized.split('.').filter(Boolean)
  const label = parts.length >= 2 ? parts.slice(-2).join('.') : normalized
  return { id: `site:${label}`, label, icon }
}

export function cityKey(geo) {
  if (!geo) return 'unknown'
  const city = String(geo.city || '').trim()
  const country = String(geo.country_code || geo.country || '').trim()
  if (city) return `${city}|${country}`.toLowerCase()
  if (country) return `country|${country}`.toLowerCase()
  if (geo.latitude != null && geo.longitude != null) {
    return `coord|${Number(geo.latitude).toFixed(1)}|${Number(geo.longitude).toFixed(1)}`
  }
  return 'unknown'
}

export function cityLabel(geo, fallback = '未知位置') {
  if (!geo) return fallback
  if (geo.city) return geo.city
  if (geo.country) return geo.country
  return fallback
}
