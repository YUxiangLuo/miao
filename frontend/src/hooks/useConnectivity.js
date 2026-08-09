import { useCallback, useEffect, useRef, useState } from 'react'
import { API_HEADERS } from '../utils.js'

export function useConnectivity() {
  const [connectivityResults, setConnectivityResults] = useState({})
  const [testingConnectivity, setTestingConnectivity] = useState(false)
  const [currentTestingSite, setCurrentTestingSite] = useState(null)
  const stopConnectivityRef = useRef(false)
  const requestControllerRef = useRef(null)

  useEffect(() => () => requestControllerRef.current?.abort(), [])

  const testSingleSite = useCallback(async (site) => {
    const controller = new AbortController()
    requestControllerRef.current?.abort()
    requestControllerRef.current = controller
    setCurrentTestingSite(site.name)

    try {
      const response = await fetch('/api/connectivity', {
        method: 'POST',
        headers: API_HEADERS,
        body: JSON.stringify({ url: site.url }),
        signal: controller.signal,
      })
      const payload = await response.json()
      if (!controller.signal.aborted) {
        setConnectivityResults((prev) => ({
          ...prev,
          [site.name]: payload.success ? payload.data : { success: false },
        }))
      }
    } catch (error) {
      if (error?.name !== 'AbortError') {
        setConnectivityResults((prev) => ({ ...prev, [site.name]: { success: false } }))
      }
    } finally {
      if (requestControllerRef.current === controller) {
        requestControllerRef.current = null
        setCurrentTestingSite(null)
      }
    }
  }, [])

  const testAllConnectivity = useCallback(async (sites) => {
    setTestingConnectivity(true)
    stopConnectivityRef.current = false
    setConnectivityResults({})
    for (const site of sites) {
      if (stopConnectivityRef.current) break
      await testSingleSite(site)
    }
    setTestingConnectivity(false)
    stopConnectivityRef.current = false
  }, [testSingleSite])

  const stopConnectivity = useCallback(() => {
    stopConnectivityRef.current = true
    requestControllerRef.current?.abort()
    requestControllerRef.current = null
    setTestingConnectivity(false)
    setCurrentTestingSite(null)
  }, [])

  const clearConnectivity = useCallback(() => {
    stopConnectivityRef.current = true
    requestControllerRef.current?.abort()
    requestControllerRef.current = null
    setConnectivityResults({})
    setTestingConnectivity(false)
    setCurrentTestingSite(null)
  }, [])

  return {
    connectivityResults,
    testingConnectivity,
    currentTestingSite,
    testSingleSite,
    testAllConnectivity,
    stopConnectivity,
    clearConnectivity,
  }
}
