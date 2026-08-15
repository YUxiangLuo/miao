import { useCallback, useState } from 'react'

const EMPTY_SNAPSHOT = {
  client: { type: 'client', name: 'This Device', geo: null },
  proxies: [],
  flows: [],
}

export function useMapSnapshot() {
  const [snapshot, setSnapshot] = useState(EMPTY_SNAPSHOT)
  const [error, setError] = useState('')

  const fetchSnapshot = useCallback(async () => {
    try {
      const response = await fetch('/api/map/snapshot')
      const payload = await response.json()
      if (!payload.success || !payload.data) {
        throw new Error(payload.message || '地图数据获取失败')
      }
      setSnapshot(payload.data)
      setError('')
      return payload.data
    } catch (error) {
      setError(error.message || '地图数据获取失败')
      return null
    }
  }, [])

  return { snapshot, error, fetchSnapshot }
}
