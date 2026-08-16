import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Leaflet 依赖真实 DOM 尺寸与栅格渲染,jsdom 下整体 mock
const mapInstance = { remove: vi.fn(), setView: vi.fn() }
// addTo 必须返回自身(与真实 Leaflet 一致),组件会把返回值存进 ref
const layerGroup = { clearLayers: vi.fn(), addTo: vi.fn(() => layerGroup) }

vi.mock('leaflet', () => {
  const chainable = () => {
    const layer = {
      addTo: vi.fn(() => layer),
      bindTooltip: vi.fn(() => layer),
    }
    return layer
  }
  return {
    default: {
      map: vi.fn(() => {
        mapInstance.setView.mockReturnValue(mapInstance)
        return { ...mapInstance, setView: vi.fn(() => mapInstance) }
      }),
      tileLayer: vi.fn(() => chainable()),
      layerGroup: vi.fn(() => layerGroup),
      marker: vi.fn(() => chainable()),
      circleMarker: vi.fn(() => chainable()),
      polyline: vi.fn(() => chainable()),
      divIcon: vi.fn((opts) => opts),
    },
  }
})

vi.mock('@turf/turf', () => ({
  greatCircle: vi.fn(() => ({
    geometry: { type: 'LineString', coordinates: [
      [112, 32],
      [120, 40],
    ] },
  })),
}))

// leaflet.css 由 vite 处理,测试中为空对象即可
vi.mock('leaflet/dist/leaflet.css', () => ({}))

import L from 'leaflet'
import { MapMode } from './MapMode.jsx'

const SAMPLE_OVERVIEW = {
  success: true,
  data: {
    running: true,
    self_point: { ip: '203.0.113.1', lat: 31.2, lng: 121.5, country: 'China', city: 'Shanghai' },
    proxy_point: { node: 'HK-Node', ip: '198.51.100.2', lat: 22.3, lng: 114.2 },
    connections: [
      { ip: '223.5.5.5', host: 'a.cn', network: 'tcp', lat: 30.5, lng: 114.3, up: 100, down: 2000, proxied: false },
      { ip: '8.8.8.8', host: 'dns.google', network: 'udp', lat: 37.4, lng: -122.1, up: 10, down: 500, proxied: true },
    ],
  },
}

function mockFetch(payload) {
  return vi.fn(() =>
    Promise.resolve({ json: () => Promise.resolve(payload) }),
  )
}

describe('MapMode', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders map container and initializes leaflet map', async () => {
    vi.stubGlobal('fetch', mockFetch(SAMPLE_OVERVIEW))
    render(<MapMode onClose={() => {}} />)

    expect(screen.getByRole('dialog', { name: '网络地图' })).toBeInTheDocument()
    expect(L.map).toHaveBeenCalled()
    expect(L.tileLayer).toHaveBeenCalled()
    await waitFor(() => expect(layerGroup.clearLayers).toHaveBeenCalled())
  })

  it('renders stats chips with direct/proxied counts from overview', async () => {
    vi.stubGlobal('fetch', mockFetch(SAMPLE_OVERVIEW))
    render(<MapMode onClose={() => {}} />)

    expect(await screen.findByText('直连 1')).toBeInTheDocument()
    expect(screen.getByText('代理 1')).toBeInTheDocument()
    expect(screen.getByText('HK-Node')).toBeInTheDocument()
    expect(screen.getByText(/203\.0\.113\.1/)).toBeInTheDocument()
  })

  it('draws markers and arcs for self, proxy and connections', async () => {
    vi.stubGlobal('fetch', mockFetch(SAMPLE_OVERVIEW))
    render(<MapMode onClose={() => {}} />)

    await waitFor(() => expect(L.circleMarker).toHaveBeenCalledTimes(2))
    // self + proxy 两个标记
    expect(L.marker).toHaveBeenCalledTimes(2)
    // self→proxy 常显弧 + 每条连接一条弧
    await waitFor(() => expect(L.polyline).toHaveBeenCalledTimes(3))
  })

  it('shows not-running hint when service is stopped', async () => {
    vi.stubGlobal('fetch', mockFetch({
      success: true,
      data: { running: false, connections: [] },
    }))
    render(<MapMode onClose={() => {}} />)

    expect(await screen.findByText(/服务未运行/)).toBeInTheDocument()
  })

  it('shows empty hint when running without connections', async () => {
    vi.stubGlobal('fetch', mockFetch({
      success: true,
      data: { running: true, connections: [] },
    }))
    render(<MapMode onClose={() => {}} />)

    expect(await screen.findByText('暂无活跃连接')).toBeInTheDocument()
  })

  it('closes via close button and Escape key', async () => {
    const user = userEvent.setup()
    vi.stubGlobal('fetch', mockFetch(SAMPLE_OVERVIEW))
    const onClose = vi.fn()
    const { unmount } = render(<MapMode onClose={onClose} />)

    await user.click(screen.getByRole('button', { name: '关闭地图模式' }))
    expect(onClose).toHaveBeenCalledTimes(1)

    unmount()
    onClose.mockClear()
    render(<MapMode onClose={onClose} />)
    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('shows retry hint when fetching fails', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new Error('network down'))))
    render(<MapMode onClose={() => {}} />)

    expect(await screen.findByText(/重试中/)).toBeInTheDocument()
  })
})
