export const READ_TIMEOUT_MS = 10_000
export const WRITE_TIMEOUT_MS = 120_000

export class RequestInterruptedError extends Error {}

export class HttpStatusError extends Error {
  readonly status: number
  constructor(status: number, message: string) {
    super(message)
    this.status = status
  }
}

/**
 * Whether a failed write request left backend execution state uncertain.
 * Harbor registers HTTP write tasks once the body arrives and finishes commit/rollback
 * even if the client times out or loses its network connection.
 */
export function isUncertainRequestError(error: unknown): error is RequestInterruptedError | TypeError {
  return error instanceof RequestInterruptedError || error instanceof TypeError
}

function parseErrorResponseMessage(rawBody: string, status: number): string {
  const body = rawBody.trim()
  if (!body) return `请求失败 (${status})`
  try {
    const parsed: unknown = JSON.parse(body)
    if (
      parsed
      && typeof parsed === 'object'
      && 'message' in parsed
      && typeof parsed.message === 'string'
      && parsed.message
    ) {
      return parsed.message
    }
  } catch {
    // Plain-text response; fall back to raw body.
  }
  return body
}

/** Bound the complete read, including body parsing. Aborting or timing out
 * settles even if a transport adapter does not implement AbortSignal. */
export async function fetchJson<T>(url: string, options: RequestInit = {}, timeout = READ_TIMEOUT_MS): Promise<T> {
  const controller = new AbortController()
  let rejectAbort!: (error: Error) => void
  const aborted = new Promise<never>((_, reject) => { rejectAbort = reject })
  const abort = () => {
    controller.abort()
    rejectAbort(new RequestInterruptedError('请求已取消或超时'))
  }
  options.signal?.addEventListener('abort', abort, { once: true })
  const timer = window.setTimeout(abort, timeout)
  try {
    if (options.signal?.aborted) abort()
    return await Promise.race([
      (async () => {
        const response = await fetch(url, { ...options, signal: controller.signal })
        if (!response.ok) {
          throw new HttpStatusError(
            response.status,
            parseErrorResponseMessage(await response.text(), response.status),
          )
        }
        return await response.json() as T
      })(),
      aborted,
    ])
  } finally {
    window.clearTimeout(timer)
    options.signal?.removeEventListener('abort', abort)
  }
}
