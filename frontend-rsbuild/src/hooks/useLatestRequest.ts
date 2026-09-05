import { useCallback, useEffect, useRef } from 'react'

/** One owner for cancellation and result publication, shared by polling and
 * action-triggered refreshes. Cleanup also invalidates already parsed results. */
export function useLatestRequest() {
  const generation = useRef(0)
  const controller = useRef<AbortController | null>(null)
  const cancel = useCallback(() => {
    generation.current++
    controller.current?.abort()
    controller.current = null
  }, [])
  const begin = useCallback(() => {
    cancel()
    const current = generation.current
    controller.current = new AbortController()
    return { signal: controller.current.signal, isCurrent: () => current === generation.current }
  }, [cancel])
  useEffect(() => cancel, [cancel])
  return { begin, cancel }
}
