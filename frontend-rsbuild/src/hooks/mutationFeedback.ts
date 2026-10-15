import type { ToastTone } from './useApi'
import type { ResourceReadResult } from './useResources'

// A successful write stays successful even when its subsequent read fails.
// Never invite repeating the write just to refresh the displayed list.
export function mutationFeedback(message: string, read: ResourceReadResult<unknown>, action: string, tone: ToastTone = 'success'): { message: string; tone: ToastTone } {
  if (read.status === 'error') {
    return { message: `${message}；列表尚未同步，请重新读取列表，勿重复${action}。`, tone: 'info' }
  }
  if (read.status === 'superseded') {
    return { message: `${message}；请查看最新列表，勿重复${action}。`, tone: 'info' }
  }
  return { message, tone }
}
