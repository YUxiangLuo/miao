import { useEffect, useRef } from 'react'
import L from 'leaflet'
import { greatCircle } from '@turf/turf'
import { X, MapPin, Satellite, ShieldCheck } from 'lucide-react'
import 'leaflet/dist/leaflet.css'
import { useMapData } from '../hooks/index.js'
import { formatBytes } from '../utils.js'

const TILE_URL = 'https://{s}.basemaps.cartocdn.com/dark_all/{z}/{x}/{y}{r}.png'
const TILE_ATTR =
  '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> &copy; <a href="https://carto.com/">CARTO</a>'

const COLOR_DIRECT = '#34d399'
const COLOR_PROXIED = '#f59e0b'
const COLOR_PROXY_HOP = '#60a5fa'

function createDotIcon(className, size = 14) {
  return L.divIcon({
    className: '',
    html: `<div class="map-dot ${className}" style="width:${size}px;height:${size}px"></div>`,
    iconSize: [size, size],
    iconAnchor: [size / 2, size / 2],
  })
}

/**
 * 大圆航线坐标;起点终点相同或跨日界线时 turf 可能返回 MultiLineString 或抛错
 * 返回 [[ [lat,lng], ... ], ...] 多条折线
 */
function arcLatLngs(from, to) {
  if (from.lat === to.lat && from.lng === to.lng) return []
  try {
    const feature = greatCircle([from.lng, from.lat], [to.lng, to.lat], { npoints: 64 })
    const geometry = feature?.geometry
    if (!geometry) return []
    const lines = geometry.type === 'MultiLineString' ? geometry.coordinates : [geometry.coordinates]
    return lines.map((line) => line.map(([lng, lat]) => [lat, lng]))
  } catch {
    return []
  }
}

function arcWeight(conn) {
  return 1 + Math.min(3, Math.log10((conn.up || 0) + (conn.down || 0) + 1) / 3)
}

function connTooltip(conn) {
  const name = conn.host || conn.ip
  const where = [conn.city, conn.country].filter(Boolean).join(', ')
  const bytes = `↑ ${formatBytes(conn.up)} · ↓ ${formatBytes(conn.down)}`
  const via = conn.proxied ? '代理' : '直连'
  return `${name}<br/>${where ? where + '<br/>' : ''}${conn.network.toUpperCase()} · ${via} · ${bytes}`
}

export function MapMode({ onClose }) {
  const containerRef = useRef(null)
  const layerGroupRef = useRef(null)
  const mapRef = useRef(null)
  const { overview, error, loaded } = useMapData(true)

  // 初始化地图(仅一次)
  useEffect(() => {
    const map = L.map(containerRef.current, {
      worldCopyJump: true,
      zoomControl: true,
      minZoom: 2,
    }).setView([32, 112], 3)
    L.tileLayer(TILE_URL, { maxZoom: 18, attribution: TILE_ATTR }).addTo(map)
    layerGroupRef.current = L.layerGroup().addTo(map)
    mapRef.current = map
    return () => {
      map.remove()
      mapRef.current = null
      layerGroupRef.current = null
    }
  }, [])

  // Esc 关闭
  useEffect(() => {
    const handleKey = (event) => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', handleKey)
    return () => window.removeEventListener('keydown', handleKey)
  }, [onClose])

  // 数据刷新 → 重绘图层
  useEffect(() => {
    const group = layerGroupRef.current
    if (!group || !overview) return
    group.clearLayers()

    const { self_point: selfPoint, proxy_point: proxyPoint, connections = [] } = overview

    if (selfPoint) {
      L.marker([selfPoint.lat, selfPoint.lng], {
        icon: createDotIcon('map-dot-self'),
        keyboard: false,
      })
        .addTo(group)
        .bindTooltip(`本机${selfPoint.ip ? ` · ${selfPoint.ip}` : ''}`, { direction: 'top' })
    }

    if (proxyPoint) {
      L.marker([proxyPoint.lat, proxyPoint.lng], {
        icon: createDotIcon('map-dot-proxy'),
        keyboard: false,
      })
        .addTo(group)
        .bindTooltip(`代理节点 · ${proxyPoint.node}`, { direction: 'top' })

      if (selfPoint) {
        arcLatLngs(selfPoint, proxyPoint).forEach((line) =>
          L.polyline(line, {
            color: COLOR_PROXY_HOP,
            weight: 2.5,
            opacity: 0.9,
            interactive: false,
          }).addTo(group)
        )
      }
    }

    connections.forEach((conn) => {
      const color = conn.proxied ? COLOR_PROXIED : COLOR_DIRECT
      L.circleMarker([conn.lat, conn.lng], {
        radius: 3.5,
        color,
        weight: 1,
        fillColor: color,
        fillOpacity: 0.85,
      })
        .addTo(group)
        .bindTooltip(connTooltip(conn))

      const anchor = conn.proxied ? proxyPoint : selfPoint
      if (anchor) {
        arcLatLngs(anchor, conn).forEach((line) =>
          L.polyline(line, {
            color,
            weight: arcWeight(conn),
            opacity: conn.proxied ? 0.4 : 0.28,
            interactive: false,
          }).addTo(group)
        )
      }
    })
  }, [overview])

  const connections = overview?.connections || []
  const directCount = connections.filter((c) => !c.proxied).length
  const proxiedCount = connections.length - directCount

  return (
    <div className="map-mode" role="dialog" aria-label="网络地图">
      <div ref={containerRef} className="map-mode-canvas" />

      <div className="map-mode-topbar">
        <div className="map-mode-title">
          <Satellite size={15} />
          <span>网络地图</span>
        </div>
        {overview?.running === false && (
          <span className="map-mode-chip map-mode-chip-warn">服务未运行,暂无连接数据</span>
        )}
        {error && <span className="map-mode-chip map-mode-chip-warn">数据获取失败,重试中…</span>}
        {overview?.running && (
          <>
            <span className="map-mode-chip">
              <MapPin size={12} />
              本机{overview.self_point?.ip ? ` ${overview.self_point.ip}` : ' 定位中…'}
            </span>
            {overview.proxy_point && (
              <span className="map-mode-chip">
                <ShieldCheck size={12} />
                {overview.proxy_point.node}
              </span>
            )}
            <span className="map-mode-chip map-mode-chip-direct">直连 {directCount}</span>
            <span className="map-mode-chip map-mode-chip-proxied">代理 {proxiedCount}</span>
          </>
        )}
        <button type="button" className="map-mode-close" onClick={onClose} aria-label="关闭地图模式">
          <X size={16} />
        </button>
      </div>

      {loaded && overview?.running && connections.length === 0 && (
        <div className="map-mode-empty">暂无活跃连接</div>
      )}
    </div>
  )
}
