import type {
  BatchNodeFailure,
  NodeSelectResult,
  RouteMode,
  StatusData,
  SubBatchResult,
  SubscriptionRefreshResult,
  SwitchProxyResult,
} from '../types/api'
import type { ToastTone } from './useApi'
import type { ResourceReadResult } from './useResources'

export interface ToastFeedback {
  message: string
  tone: ToastTone
}

// A successful write stays successful even when its subsequent read fails.
// Never invite repeating the write just to refresh the displayed list.
export function mutationFeedback(
  message: string,
  read: ResourceReadResult<unknown>,
  action: string,
  tone: ToastTone = 'success',
): ToastFeedback {
  if (read.status === 'error') {
    return { message: `${message}；列表尚未同步，请重新读取列表，勿重复${action}。`, tone: 'info' }
  }
  if (read.status === 'superseded') {
    return { message: `${message}；请查看最新列表，勿重复${action}。`, tone: 'info' }
  }
  return { message, tone }
}

/**
 * Classifies node-selection updates by structured strategy fields rather than
 * prose messages. Returns null when `useAppData`'s status-warning effect already
 * surfaces the backend's regional fallback warning, avoiding duplicate toasts.
 */
export function nodeSelectFeedback(
  result: NodeSelectResult | undefined,
  statusRead: ResourceReadResult<StatusData>,
): ToastFeedback | null {
  if (!result) {
    return { message: '节点选择请求已处理，请查看当前策略', tone: 'info' }
  }
  const fellBackToManual = result.requested !== 'manual' && result.node_select === 'manual'
  if (fellBackToManual) {
    const statusAlreadyShowsWarning = statusRead.status === 'success' && Boolean(statusRead.data.warning)
    return statusAlreadyShowsWarning
      ? null
      : { message: '请求的地区暂无可用节点，当前使用手动选择', tone: 'info' }
  }
  return { message: '节点选择已更新', tone: 'success' }
}

export function switchProxyFeedback(
  nodeName: string,
  result: SwitchProxyResult | undefined,
): ToastFeedback {
  if (result?.persisted) {
    return { message: `已切换到 ${nodeName}`, tone: 'success' }
  }
  return {
    message: `已切换到 ${nodeName}，但保存选择失败，重启后可能恢复旧节点`,
    tone: 'error',
  }
}

export function vergeImportFeedback(
  result: SubBatchResult | undefined,
  read: ResourceReadResult<unknown>,
): ToastFeedback {
  if (!result) {
    return mutationFeedback('订阅导入请求已完成，请查看订阅列表和节点获取状态。', read, '导入', 'info')
  }
  return mutationFeedback(
    `已新增 ${result.added} 条订阅地址，跳过 ${result.skipped} 条已有订阅。请查看节点获取状态。`,
    read,
    '导入',
    result.added ? 'success' : 'info',
  )
}

export function subscriptionRefreshFeedback(
  result: SubscriptionRefreshResult | undefined,
): ToastFeedback {
  if (!result) {
    return { message: '刷新请求已处理；后端未提供详细结果，请查看订阅状态。', tone: 'info' }
  }
  if (!result.fetch_succeeded) {
    return { message: '订阅获取失败，已保留原配置。请查看订阅错误详情。', tone: 'error' }
  }
  if (result.report?.failed_sources > 0) {
    return {
      message: `订阅部分获取成功：成功 ${result.report.successful_sources} 个，失败 ${result.report.failed_sources} 个。请查看订阅详情。${result.warning || ''}`,
      tone: 'error',
    }
  }
  if (result.warning) {
    return { message: `订阅刷新有告警：${result.warning}`, tone: 'error' }
  }
  return {
    message: result.runtime_updated ? '订阅已获取，代理配置已更新' : '订阅已获取，代理运行状态未改变',
    tone: 'success',
  }
}

export function batchNodeFailuresFeedback(failed: readonly BatchNodeFailure[]): ToastFeedback | null {
  if (failed.length === 0) return null
  const failures = failed.map((item) => `${item.tag}: ${item.message}`)
  const shown = failures.slice(0, 2).join('; ')
  const more = failures.length > 2 ? ` 等 ${failures.length} 项` : ''
  return { message: `导入失败: ${shown}${more}`, tone: 'error' }
}

export function routeModeConfirmCopy(nextMode: RouteMode): { title: string; message: string } {
  if (nextMode === 'global') {
    return {
      title: '切换为全局代理',
      message: '关闭内置的内网与大陆直连分流，先按自定义规则匹配，其余流量全部走默认代理。切换时连接会短暂重连。',
    }
  }
  return {
    title: '切换为规则分流',
    message: '内网与中国大陆流量优先直连，再匹配自定义规则，其余流量走默认代理。切换时连接会短暂重连。',
  }
}
