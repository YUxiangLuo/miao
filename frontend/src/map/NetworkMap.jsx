import { useEffect, useMemo, useRef, useState } from 'react'
import { MapPin, Maximize, Radio, Search, X, ZoomIn, ZoomOut } from 'lucide-react'
import { classNames, formatBytes, formatSpeed, getDelayTone } from '../utils.js'
import { WORLD_LAND } from './worldLand.js'
import {
  MAP_HEIGHT,
  MAP_WIDTH,
  flowPathData,
  jitterGeo,
  project,
} from './projection.js'
import { initialViewBox, MAX_SCALE, panBy, scaleOf, zoomAtPoint } from './zoom.js'
import {
  aggregateDestinationGroups,
  aggregatePaths,
  aggregateProxyCities,
  filterFlows,
  routeCounts,
} from './aggregate.js'
import {
  STARFIELD,
  allocateParticles,
  flowWidth,
  particleDuration,
} from './visuals.js'

const ROUTE_FILTERS = [
  { value: 'all', label: '全部' },
  { value: 'direct', label: '直连' },
  { value: 'proxy', label: '代理' },
]

const PROTOCOL_FILTERS = [
  { value: 'all', label: '全部' },
  { value: 'tcp', label: 'TCP' },
  { value: 'udp', label: 'UDP' },
]

const ACTIVE_FILTERS = [
  { value: 'all', label: '全部' },
  { value: 'active', label: '活跃' },
  { value: 'idle', label: '空闲' },
]

const LAND_POLYGONS = WORLD_LAND.map((ring) => ring.map(([lng, lat]) => {
  const point = project(lng, lat)
  return `${point.x.toFixed(1)},${point.y.toFixed(1)}`
}).join(' '))

// 经纬网：等距圆柱投影下就是水平/垂直直线
const GRATICULE_LATITUDES = [-60, -30, 0, 30, 60].map((lat) => project(0, lat).y)
const GRATICULE_LONGITUDES = [-150, -120, -90, -60, -30, 0, 30, 60, 90, 120, 150]
  .map((lng) => project(lng, 0).x)

function viewBoxString(viewBox) {
  return [viewBox.x, viewBox.y, viewBox.w, viewBox.h]
    .map((value) => Number(value.toFixed(2)))
    .join(' ')
}

function flowElementId(pathId) {
  return `flow-${String(pathId).replace(/[^a-zA-Z0-9]/g, '-')}`
}

function markerPoint(geo) {
  if (!geo || geo.latitude == null || geo.longitude == null) return null
  return project(geo.longitude, geo.latitude)
}

function FilterGroup({ label, value, options, counts, onChange }) {
  return (
    <div className="map-filter-group" role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={classNames('map-filter-chip', value === option.value && 'active')}
          aria-pressed={value === option.value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
          {counts && counts[option.value] != null && (
            <span className="map-filter-count">{counts[option.value]}</span>
          )}
        </button>
      ))}
    </div>
  )
}

function DelayText({ delay }) {
  if (delay == null) return <span className="delay-unknown">--</span>
  if (delay < 0) return <span className="delay-timeout">超时</span>
  return <span className={classNames('delay-value', getDelayTone(delay))}>{delay} ms</span>
}

export function NetworkMap({
  snapshot,
  error,
  status,
  delays = {},
  testingNodes = {},
  switchingNode = '',
  primaryGroupName,
  currentNodeName = '',
  onSwitchProxy,
  onTestDelay,
}) {
  const [route, setRoute] = useState('all')
  const [protocol, setProtocol] = useState('all')
  const [active, setActive] = useState('all')
  const [query, setQuery] = useState('')
  const [hovered, setHovered] = useState(null)
  const [selected, setSelected] = useState(null)
  const [viewBox, setViewBox] = useState(initialViewBox)
  const [panning, setPanning] = useState(false)
  const svgRef = useRef(null)
  const panRef = useRef(null)
  const didDragRef = useRef(false)

  const scale = scaleOf(viewBox)

  // React 的 onWheel 是 passive 的，无法 preventDefault，需要原生监听
  useEffect(() => {
    const svg = svgRef.current
    if (!svg) return undefined
    const onWheel = (event) => {
      event.preventDefault()
      const rect = svg.getBoundingClientRect()
      if (!rect.width || !rect.height) return
      const factor = Math.exp(-event.deltaY * 0.0015)
      setViewBox((current) => {
        const cx = current.x + ((event.clientX - rect.left) / rect.width) * current.w
        const cy = current.y + ((event.clientY - rect.top) / rect.height) * current.h
        return zoomAtPoint(current, cx, cy, factor)
      })
    }
    svg.addEventListener('wheel', onWheel, { passive: false })
    return () => svg.removeEventListener('wheel', onWheel)
  }, [])

  const zoomBy = (factor) => {
    setViewBox((current) => zoomAtPoint(
      current,
      current.x + current.w / 2,
      current.y + current.h / 2,
      factor,
    ))
  }

  const onPointerDown = (event) => {
    // 新的按下手势开始时重置拖拽标记，避免上次中断的拖拽吞掉本次点击
    didDragRef.current = false
    if (scaleOf(viewBox) <= 1 || event.button !== 0) return
    const rect = svgRef.current?.getBoundingClientRect()
    if (!rect || !rect.width) return
    // 注意：这里不做 setPointerCapture。一旦捕获，后续的 click 会被重定向到
    // svg 本身，标记点的 onClick 就收不到了（卡片打不开）。等真正拖起来再捕获。
    panRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      origin: viewBox,
      rect,
      captured: false,
    }
  }

  const onPointerMove = (event) => {
    const pan = panRef.current
    if (!pan || event.pointerId !== pan.pointerId) return
    const dxPixels = event.clientX - pan.startX
    const dyPixels = event.clientY - pan.startY
    if (!pan.captured) {
      if (Math.abs(dxPixels) + Math.abs(dyPixels) <= 3) return
      event.currentTarget.setPointerCapture(event.pointerId)
      pan.captured = true
      didDragRef.current = true
      setPanning(true)
    }
    setViewBox(panBy(
      pan.origin,
      (-dxPixels / pan.rect.width) * pan.origin.w,
      (-dyPixels / pan.rect.height) * pan.origin.h,
    ))
  }

  const onPointerUp = (event) => {
    if (panRef.current?.pointerId !== event.pointerId) return
    panRef.current = null
    setPanning(false)
  }

  const swallowDragClick = () => {
    if (didDragRef.current) {
      didDragRef.current = false
      return true
    }
    return false
  }

  const client = snapshot?.client
  const allFlows = useMemo(() => snapshot?.flows || [], [snapshot?.flows])
  const filteredFlows = useMemo(
    () => filterFlows(allFlows, { route, protocol, active, query }),
    [allFlows, route, protocol, active, query],
  )
  const counts = useMemo(() => routeCounts(allFlows), [allFlows])
  const destinations = useMemo(
    () => aggregateDestinationGroups(filteredFlows),
    [filteredFlows],
  )
  const proxyCities = useMemo(
    () => aggregateProxyCities(snapshot?.proxies || [], filteredFlows, delays),
    [snapshot?.proxies, filteredFlows, delays],
  )
  const paths = useMemo(() => aggregatePaths(destinations), [destinations])

  const clientPoint = markerPoint(client?.geo)
  const locatedDestinations = destinations.filter((item) => item.located)
  const unlocatedDestinations = destinations.filter((item) => !item.located)
  const locatedProxies = proxyCities.filter((item) => item.located)

  const destPositions = useMemo(() => {
    const positions = new Map()
    for (const dest of locatedDestinations) {
      positions.set(dest.id, jitterGeo(dest.geo, dest.id))
    }
    return positions
  }, [locatedDestinations])

  const renderedPaths = useMemo(
    () => paths
      .map((path) => {
        const destGeo = destPositions.get(path.destId)
        const segments = flowPathData(client?.geo, destGeo, path.proxyGeo, path.direct)
        const speed = path.downloadSpeed + path.uploadSpeed
        return {
          ...path,
          segments,
          speed,
          width: flowWidth(speed),
          elementId: flowElementId(path.id),
        }
      })
      .filter((path) => path.segments.length > 0),
    [paths, destPositions, client?.geo],
  )

  const particleBudget = useMemo(
    () => allocateParticles(
      renderedPaths.map((path) => ({ id: path.id, speed: path.active ? path.speed : 0 })),
    ),
    [renderedPaths],
  )

  const selectedDest = selected?.kind === 'destination'
    ? destinations.find((item) => item.id === selected.id)
    : null
  const selectedProxy = selected?.kind === 'proxy'
    ? proxyCities.find((item) => item.id === selected.id)
    : null

  const hoverDest = hovered?.kind === 'destination'
    ? destinations.find((item) => item.id === hovered.id)
    : null

  return (
    <section className="network-map" aria-label="世界网络地图">
      <div className="map-toolbar">
        <FilterGroup
          label="路径"
          value={route}
          options={ROUTE_FILTERS}
          counts={{ all: counts.all, direct: counts.direct, proxy: counts.proxy }}
          onChange={setRoute}
        />
        <FilterGroup
          label="协议"
          value={protocol}
          options={PROTOCOL_FILTERS}
          counts={{ all: counts.all, tcp: counts.tcp, udp: counts.udp }}
          onChange={setProtocol}
        />
        <FilterGroup
          label="状态"
          value={active}
          options={ACTIVE_FILTERS}
          counts={{ all: counts.all, active: counts.active, idle: counts.idle }}
          onChange={setActive}
        />
        <label className="map-search">
          <Search size={14} />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索站点、IP、城市、节点"
          />
        </label>
      </div>

      <div className="map-stage">
        <svg
          ref={svgRef}
          className={classNames('map-canvas', panning && 'is-panning')}
          viewBox={viewBoxString(viewBox)}
          role="img"
          aria-label="实时网络路径世界地图"
          onClick={() => {
            if (swallowDragClick()) return
            setSelected(null)
          }}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        >
          <defs>
            <radialGradient id="map-ocean-gradient" cx="50%" cy="38%" r="80%">
              <stop offset="0%" stopColor="#0d1526" />
              <stop offset="100%" stopColor="#05070c" />
            </radialGradient>
            <linearGradient id="map-land-gradient" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#1b2640" />
              <stop offset="100%" stopColor="#141c30" />
            </linearGradient>
            <filter id="map-flow-glow" x="-20%" y="-20%" width="140%" height="140%">
              <feGaussianBlur stdDeviation="2.5" result="blur" />
              <feMerge>
                <feMergeNode in="blur" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>

          <rect width={MAP_WIDTH} height={MAP_HEIGHT} className="map-ocean" />

          {STARFIELD.map((star) => (
            <circle
              key={star.id}
              className="map-star"
              cx={star.x}
              cy={star.y}
              r={star.r}
              opacity={star.opacity}
            />
          ))}

          {GRATICULE_LATITUDES.map((y) => (
            <line
              key={`lat-${y}`}
              className="map-graticule"
              x1="0" y1={y} x2={MAP_WIDTH} y2={y}
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {GRATICULE_LONGITUDES.map((x) => (
            <line
              key={`lng-${x}`}
              className="map-graticule"
              x1={x} y1="0" x2={x} y2={MAP_HEIGHT}
              vectorEffect="non-scaling-stroke"
            />
          ))}

          {LAND_POLYGONS.map((points, index) => (
            <polygon key={index} className="map-land" points={points} vectorEffect="non-scaling-stroke" />
          ))}

          {/* 空闲连接垫底，无滤镜 */}
          <g>
            {renderedPaths.filter((path) => !path.active).map((path) => (
              <g
                key={path.id}
                className={classNames('map-flow', path.direct ? 'direct' : 'proxy', path.network)}
              >
                {path.segments.map((d, index) => (
                  <path
                    key={d}
                    id={`${path.elementId}-${index}`}
                    d={d}
                    className="map-flow-line"
                    style={{ strokeWidth: path.width }}
                    vectorEffect="non-scaling-stroke"
                  />
                ))}
              </g>
            ))}
          </g>

          {/* 活跃连接：发光 + 沿线流动粒子 */}
          <g filter="url(#map-flow-glow)">
            {renderedPaths.filter((path) => path.active).map((path) => {
              const particleTotal = particleBudget.get(path.id) || 0
              const duration = particleDuration(path.speed)
              return (
                <g
                  key={path.id}
                  className={classNames(
                    'map-flow',
                    path.direct ? 'direct' : 'proxy',
                    path.network,
                    'active',
                  )}
                >
                  {path.segments.map((d, index) => (
                    <path
                      key={d}
                      id={`${path.elementId}-${index}`}
                      d={d}
                      className="map-flow-line"
                      style={{ strokeWidth: path.width }}
                      vectorEffect="non-scaling-stroke"
                    />
                  ))}
                  {particleTotal > 0 && path.segments.map((_, segmentIndex) => (
                    Array.from({ length: particleTotal }, (__, particleIndex) => (
                      <circle
                        key={`${segmentIndex}-${particleIndex}`}
                        className="map-flow-particle"
                        r={2 / scale}
                      >
                        <animateMotion
                          dur={`${duration.toFixed(2)}s`}
                          begin={`${(-duration * particleIndex / particleTotal).toFixed(2)}s`}
                          repeatCount="indefinite"
                        >
                          <mpath href={`#${path.elementId}-${segmentIndex}`} />
                        </animateMotion>
                      </circle>
                    ))
                  ))}
                </g>
              )
            })}
          </g>

          {locatedDestinations.map((group) => {
            const geo = destPositions.get(group.id)
            const point = markerPoint(geo)
            if (!point) return null
            return (
              <g
                key={group.id}
                className={classNames('map-marker destination', group.active && 'is-active')}
                transform={`translate(${point.x} ${point.y}) scale(${1 / scale})`}
                role="button"
                tabIndex={0}
                aria-label={`${group.service.label} · ${group.city}`}
                onMouseEnter={() => setHovered({ kind: 'destination', id: group.id })}
                onMouseLeave={() => setHovered(null)}
                onClick={(event) => {
                  event.stopPropagation()
                  if (swallowDragClick()) return
                  setHovered(null)
                  setSelected({ kind: 'destination', id: group.id })
                }}
              >
                <circle r={group.count > 8 ? 7 : 5.5} />
                {group.active && <circle r="7" className="map-ping destination-ping" />}
                {group.count > 1 && (
                  <text y="12" textAnchor="middle" className="map-marker-count">{group.count}</text>
                )}
              </g>
            )
          })}

          {/* 代理标记最后渲染，保证拥挤区域里不被目标点盖住、总能点到 */}
          {locatedProxies.map((group) => {
            const point = markerPoint(group.geo)
            if (!point) return null
            return (
              <g
                key={group.id}
                className={classNames('map-marker proxy', group.activeNode && 'is-active')}
                transform={`translate(${point.x} ${point.y}) scale(${1 / scale})`}
                role="button"
                tabIndex={0}
                aria-label={`${group.city} × ${group.count}`}
                onClick={(event) => {
                  event.stopPropagation()
                  if (swallowDragClick()) return
                  setHovered(null)
                  setSelected({ kind: 'proxy', id: group.id })
                }}
              >
                <circle r="12" fill="transparent" />
                <polygon points="0,-9 8,0 0,9 -8,0" />
                {group.activeNode && <circle r="9" className="map-ping proxy-ping" />}
              </g>
            )
          })}

          {clientPoint && (
            <g
              className="map-marker client"
              transform={`translate(${clientPoint.x} ${clientPoint.y}) scale(${1 / scale})`}
            >
              <circle r="7" className="map-ping client-ping" />
              <circle r="7" className="map-client-ring" />
              <circle r="3.5" />
              <title>{client?.name || 'YOU'}</title>
            </g>
          )}
        </svg>

        <div className="map-zoom-controls">
          <button
            type="button"
            className="map-zoom-button"
            aria-label="放大"
            disabled={scale >= MAX_SCALE}
            onClick={() => zoomBy(1.6)}
          >
            <ZoomIn size={14} />
          </button>
          <button
            type="button"
            className="map-zoom-button"
            aria-label="缩小"
            disabled={scale <= 1}
            onClick={() => zoomBy(1 / 1.6)}
          >
            <ZoomOut size={14} />
          </button>
          {scale > 1 && (
            <button
              type="button"
              className="map-zoom-button"
              aria-label="重置缩放"
              onClick={() => setViewBox(initialViewBox())}
            >
              <Maximize size={14} />
            </button>
          )}
        </div>

        <div className="map-legend" aria-hidden="true">
          <span><i className="map-swatch client" /> {client?.name || 'YOU'}</span>
          <span><i className="map-swatch proxy" /> 代理</span>
          <span><i className="map-swatch destination" /> 目标</span>
          <span className="map-swatch-line solid">TCP</span>
          <span className="map-swatch-line dashed">UDP</span>
        </div>

        {!clientPoint && !locatedProxies.length && !locatedDestinations.length && (
          <div className="map-empty">
            {error || (status?.running
              ? '正在定位本机与远端位置…'
              : '启动服务后，这里会显示本机、代理出口和远端目标的实时路径')}
          </div>
        )}

        {(!clientPoint && (locatedProxies.length > 0 || locatedDestinations.length > 0)
          || unlocatedDestinations.length > 0) && (
          <div className="map-unlocated">
            {!clientPoint && (locatedProxies.length > 0 || locatedDestinations.length > 0)
              ? '本机未定位'
              : null}
            {unlocatedDestinations.length > 0
              ? `${!clientPoint && (locatedProxies.length > 0 || locatedDestinations.length > 0) ? ' · ' : ''}未定位 ${unlocatedDestinations.length} 个目标`
              : null}
          </div>
        )}

        {hoverDest && (
          <div className="map-tooltip" role="status">
            <div className="map-tooltip-title">{hoverDest.service.label}</div>
            <div className="map-tooltip-meta">
              {hoverDest.city}{hoverDest.country ? `, ${hoverDest.country}` : ''}
            </div>
            {hoverDest.flows[0]?.destination?.domain
              && hoverDest.flows[0].destination.domain !== hoverDest.flows[0].destination.ip && (
              <div className="map-tooltip-meta">{hoverDest.flows[0].destination.domain}</div>
            )}
            {hoverDest.flows[0]?.destination?.ip && (
              <div className="map-tooltip-meta">{hoverDest.flows[0].destination.ip}</div>
            )}
            <div className="map-tooltip-meta">
              {String(hoverDest.flows[0]?.network || 'tcp').toUpperCase()}
              {hoverDest.flows[0]?.port ? ` :${hoverDest.flows[0].port}` : ''}
            </div>
            <div className="map-tooltip-stats">
              <span>↓ {formatSpeed(hoverDest.downloadSpeed)}</span>
              <span>↑ {formatSpeed(hoverDest.uploadSpeed)}</span>
              <span>{hoverDest.count} 条连接</span>
            </div>
            <div className="map-tooltip-route">
              {hoverDest.flows.some((flow) => flow.proxy)
                ? `PROXY → ${hoverDest.flows.find((flow) => flow.proxy)?.proxy?.name || '代理'}`
                : 'DIRECT'}
            </div>
          </div>
        )}
      </div>

      {selectedDest && (
        <aside className="map-detail" aria-label="连接详情">
          <div className="map-detail-head">
            <div>
              <div className="map-detail-kicker">目标</div>
              <h3>{selectedDest.service.label}</h3>
              <p>{selectedDest.city}{selectedDest.country ? ` · ${selectedDest.country}` : ''}</p>
            </div>
            <button type="button" className="icon-button" onClick={() => setSelected(null)} aria-label="关闭">
              <X size={16} />
            </button>
          </div>
          <div className="map-detail-stats">
            <span>↓ {formatSpeed(selectedDest.downloadSpeed)}</span>
            <span>↑ {formatSpeed(selectedDest.uploadSpeed)}</span>
            <span>{formatBytes(selectedDest.downloadTotal + selectedDest.uploadTotal)}</span>
          </div>
          <ul className="map-detail-list">
            {selectedDest.flows.map((flow) => (
              <li key={flow.id}>
                <div className="map-detail-row">
                  <strong>{flow.destination.domain || flow.destination.ip}</strong>
                  <span>{String(flow.network || 'tcp').toUpperCase()}{flow.port ? ` :${flow.port}` : ''}</span>
                </div>
                <div className="map-detail-row muted">
                  <span>{flow.destination.ip}</span>
                  <span>{flow.proxy ? flow.proxy.name : 'DIRECT'}</span>
                </div>
                <div className="map-detail-row muted">
                  <span>↓ {formatSpeed(flow.download_speed)} · ↑ {formatSpeed(flow.upload_speed)}</span>
                  <span>{flow.rule || '—'}</span>
                </div>
              </li>
            ))}
          </ul>
        </aside>
      )}

      {selectedProxy && (
        <aside className="map-detail" aria-label="代理城市">
          <div className="map-detail-head">
            <div>
              <div className="map-detail-kicker">代理城市</div>
              <h3>{selectedProxy.city} × {selectedProxy.count}</h3>
              <p>
                {currentNodeName && selectedProxy.nodes.some((node) => node.name === currentNodeName)
                  ? `当前 ${currentNodeName}`
                  : selectedProxy.activeNode
                    ? `承载 ${selectedProxy.activeNode}`
                    : '未承载活动连接'}
                {selectedProxy.bestDelay ? ` · 最快 ${selectedProxy.bestDelay} ms` : ''}
              </p>
            </div>
            <button type="button" className="icon-button" onClick={() => setSelected(null)} aria-label="关闭">
              <X size={16} />
            </button>
          </div>
          <ul className="map-detail-list">
            {selectedProxy.nodes.map((node) => {
              const current = Boolean(currentNodeName) && node.name === currentNodeName
              return (
                <li key={node.name} className={classNames(current && 'is-current')}>
                  <div className="map-detail-row">
                    <strong>{node.name}</strong>
                    <DelayText delay={node.delay} />
                  </div>
                  <div className="map-detail-row muted">
                    <span>{node.server}</span>
                    <span className="map-node-actions">
                      <button
                        type="button"
                        className="text-button"
                        disabled={Boolean(testingNodes[node.name])}
                        onClick={() => onTestDelay?.(node.name)}
                      >
                        <Radio size={12} /> 测速
                      </button>
                      <button
                        type="button"
                        className="text-button"
                        disabled={!primaryGroupName || switchingNode === node.name || current}
                        onClick={() => onSwitchProxy?.(primaryGroupName, node.name)}
                      >
                        <MapPin size={12} /> {current ? '使用中' : '切换'}
                      </button>
                    </span>
                  </div>
                </li>
              )
            })}
          </ul>
        </aside>
      )}
    </section>
  )
}
