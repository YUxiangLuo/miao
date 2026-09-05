export const READ_TIMEOUT_MS = 10_000

/** Bound the complete read, including body parsing. Aborting or timing out
 * settles even if a transport adapter does not implement AbortSignal. */
export async function fetchJson<T>(url: string, options: RequestInit = {}, timeout = READ_TIMEOUT_MS): Promise<T> {
  const controller = new AbortController()
  let rejectAbort!: (error: Error) => void
  const aborted = new Promise<never>((_, reject) => { rejectAbort = reject })
  const abort = () => {
    controller.abort()
    rejectAbort(new Error('请求已取消或超时'))
  }
  options.signal?.addEventListener('abort', abort, { once: true })
  const timer = window.setTimeout(abort, timeout)
  try {
    if (options.signal?.aborted) abort()
    return await Promise.race([
      (async () => {
        const response = await fetch(url, { ...options, signal: controller.signal })
        if (!response.ok) throw new Error((await response.text()).trim() || `请求失败 (${response.status})`)
        return await response.json() as T
      })(),
      aborted,
    ])
  } finally {
    window.clearTimeout(timer)
    options.signal?.removeEventListener('abort', abort)
  }
}
