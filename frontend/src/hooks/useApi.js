import { useCallback, useRef, useState } from 'react'
import { API_HEADERS } from '../utils.js'

export function useToast() {
  const [toasts, setToasts] = useState([])
  const toastIdRef = useRef(0)

  const showToast = useCallback((message, tone = 'info') => {
    const id = ++toastIdRef.current
    setToasts((prev) => [...prev, { id, message, tone }])
    window.setTimeout(() => {
      setToasts((prev) => prev.filter((item) => item.id !== id))
    }, 3500)
  }, [])

  return { toasts, showToast }
}

export function useApi({ setLoadingAction }) {
  const apiCall = useCallback(async (endpoint, options = {}, action = '') => {
    setLoadingAction(action)
    try {
      const response = await fetch(`/api/${endpoint}`, { headers: API_HEADERS, ...options })
      const payload = await response.json()
      if (!response.ok || !payload.success) throw new Error(payload.message || '请求失败')
      return payload
    } finally {
      setLoadingAction('')
    }
  }, [setLoadingAction])

  return { apiCall }
}
