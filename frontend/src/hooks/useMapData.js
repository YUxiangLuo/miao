import { useCallback, useState } from 'react'
import { usePolling } from './usePolling.js'

/**
 * 地图模式数据:仅在地图片打开时轮询 /api/map/overview
 * 后端返回 { running, self_point, proxy_point, connections[] }
 */
export function useMapData(active) {
  const [overview, setOverview] = useState(null)
  const [error, setError] = useState('')
  const [loaded, setLoaded] = useState(false)

  const fetchOverview = useCallback(async () => {
    try {
      const response = await fetch('/api/map/overview')
      const payload = await response.json()
      if (!payload.success) {
        throw new Error(payload.message || '获取地图数据失败')
      }
      setOverview(payload.data || null)
      setError('')
    } catch (err) {
      setError(err?.message || '获取地图数据失败')
    } finally {
      setLoaded(true)
    }
  }, [])

  usePolling(active ? [fetchOverview] : [], active)

  return { overview, error, loaded, refresh: fetchOverview }
}
