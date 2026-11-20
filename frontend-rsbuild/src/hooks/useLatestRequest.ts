import { onCleanup } from 'solid-js'

/** One owner for cancellation and result publication, shared by polling and
 * action-triggered refreshes. Cleanup also invalidates already parsed results. */
export function useLatestRequest() {
  let generation = 0
  let controller: AbortController | null = null

  const cancel = () => {
    generation++
    controller?.abort()
    controller = null
  }

  const begin = () => {
    cancel()
    const current = generation
    controller = new AbortController()
    return { signal: controller.signal, isCurrent: () => current === generation }
  }

  onCleanup(cancel)

  return { begin, cancel }
}
