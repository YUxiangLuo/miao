import { createSignal, onSettled } from 'solid-js'
import { CONNECTIONS_MODAL_MIN_WIDTH } from '../tokens'

export function useDesktopLayout() {
  const mediaQuery = window.matchMedia(`(max-width: ${CONNECTIONS_MODAL_MIN_WIDTH - 1}px)`)
  const [isDesktop, setIsDesktop] = createSignal(!mediaQuery.matches)

  onSettled(() => {
    const handleChange = () => setIsDesktop(!mediaQuery.matches)
    mediaQuery.addEventListener('change', handleChange)
    return () => mediaQuery.removeEventListener('change', handleChange)
  })

  return isDesktop
}
