import { useEffect, useState } from 'react'
import { CONNECTIONS_MODAL_MIN_WIDTH } from '../layout.js'

export function useDesktopLayout() {
  const [isDesktop, setIsDesktop] = useState(() => (
    !window.matchMedia(`(max-width: ${CONNECTIONS_MODAL_MIN_WIDTH - 1}px)`).matches
  ))

  useEffect(() => {
    const mediaQuery = window.matchMedia(`(max-width: ${CONNECTIONS_MODAL_MIN_WIDTH - 1}px)`)
    const handleChange = () => setIsDesktop(!mediaQuery.matches)

    handleChange()
    mediaQuery.addEventListener('change', handleChange)
    return () => mediaQuery.removeEventListener('change', handleChange)
  }, [])

  return isDesktop
}
