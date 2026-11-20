import { createSignal, onCleanup } from 'solid-js'
import { CONNECTIONS_MODAL_MIN_WIDTH } from '../tokens'

export function useDesktopLayout() {
  const mediaQuery = window.matchMedia(`(max-width: ${CONNECTIONS_MODAL_MIN_WIDTH - 1}px)`)
  const [isDesktop, setIsDesktop] = createSignal(!mediaQuery.matches)

  const handleChange = () => setIsDesktop(!mediaQuery.matches)
  mediaQuery.addEventListener('change', handleChange)
  onCleanup(() => mediaQuery.removeEventListener('change', handleChange))

  return isDesktop
}
