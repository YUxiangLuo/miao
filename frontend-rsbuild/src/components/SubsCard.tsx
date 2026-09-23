import { memo, useEffect, useId, useRef, useState } from 'react'
import { Check, CircleX, Clock, RefreshCw, Rss, Plus, Trash2, X } from 'lucide-react'
import { ICON } from '../tokens'
import { Button, SectionCard } from './ui'
import { useDialog } from '../hooks/useDialog'
import { classNames, maskSubscription } from '../utils'
import { SubDetailModal } from './SubDetailModal'
import { ScheduleModal } from './ScheduleModal'
import { ResourceError } from './ResourceError'
import type { ScheduledRefreshRequest, SubStatus, SubscriptionRefreshStatus } from '../types/api'

interface SubRowProps {
  sub: SubStatus
  onDelete: (url: string) => void
  onShowNodes: (sub: SubStatus) => void
  disabled: boolean
}

const SubRow = memo(function SubRow({ sub, onDelete, onShowNodes, disabled }: SubRowProps) {
  const state = sub.state || (sub.success ? 'ready' : 'failed')
  const pending = state === 'pending' || state === 'refreshing'
  // 节点管理读取本地快照，不依赖本轮刷新成功；空列表也可能有失配禁用待清理。
  const showFetchStatus = pending || !sub.success || sub.node_count === 0
  return (
    <div className="list-row">
      <div className={classNames('status-icon-badge', pending ? 'info' : sub.success ? 'success' : 'error')}>
        {pending
          ? <RefreshCw size={ICON.xs} className={state === 'refreshing' ? 'spin' : undefined} />
          : sub.success
            ? <Check size={ICON.xs} />
            : <CircleX size={ICON.xs} />}
      </div>
      <div className="list-row-content">
        <div className="list-row-title">{maskSubscription(sub.url)}</div>
        <button
          type="button"
          className="list-row-meta meta-link"
          title="查看订阅节点"
          onClick={() => onShowNodes(sub)}
        >
          {sub.node_count > 0
            ? `${sub.node_count} 个节点${sub.disabled_count > 0 ? ` · 禁用 ${sub.disabled_count}` : ''}`
            : '查看节点'}
        </button>
        {showFetchStatus && (
          <div
            className={classNames('list-row-meta', state === 'failed' && 'error')}
            title={state === 'failed' ? sub.error : undefined}
          >
            {state === 'pending'
              ? '等待首次获取'
              : state === 'refreshing'
                ? sub.success ? `正在刷新，上次获取 ${sub.node_count} 个节点` : '正在获取订阅'
                : sub.success
                  ? sub.node_count > 0 ? `${sub.node_count} 个节点` : '获取成功，暂无代理节点'
                  : sub.error || '获取失败'}
          </div>
        )}
        {!pending && !sub.success && sub.node_count > 0 && (
          <div className="list-row-meta subscription-feedback">保留上次获取的 {sub.node_count} 个缓存节点，不代表本次获取成功</div>
        )}
      </div>
      <button
        className="icon-button subtle"
        onClick={() => onDelete(sub.url)}
        disabled={disabled}
        aria-label={`删除订阅 ${maskSubscription(sub.url)}`}
      >
        <Trash2 size={ICON.xs} />
      </button>
    </div>
  )
})

interface AddSubModalProps {
  open: boolean
  loading: boolean
  onClose: () => void
  /** 返回是否添加成功；成功时由本组件负责关闭并清空输入 */
  onSubmit: (url: string) => Promise<boolean>
}

function AddSubModal({ open, loading, onClose, onSubmit }: AddSubModalProps) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [url, setUrl] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const inFlight = useRef(false)
  const generation = useRef(0)
  const busy = loading || submitting

  // 关闭后重新打开时回到空输入
  useEffect(() => {
    if (!open) {
      setUrl('')
      generation.current++
    }
  }, [open])

  if (!open) return null

  const submit = async () => {
    const trimmed = url.trim()
    if (!trimmed || busy || inFlight.current) return
    inFlight.current = true
    setSubmitting(true)
    const submittedGeneration = generation.current
    try {
      if (await onSubmit(trimmed) && submittedGeneration === generation.current) onClose()
    } finally {
      inFlight.current = false
      setSubmitting(false)
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card modal-confirm"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Rss size={ICON.lg} className="icon-accent" />
            <h3 id={titleId}>添加订阅</h3>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭添加订阅对话框">
            <X size={ICON.md} />
          </button>
        </div>
        <div className="field add-sub-field">
          <input
            value={url}
            disabled={busy}
            onChange={(event) => setUrl(event.target.value)}
            onKeyDown={(event) => event.key === 'Enter' && submit()}
            placeholder="粘贴订阅链接..."
            aria-label="订阅链接"
            data-autofocus
          />
        </div>
        <p className="list-row-meta subscription-feedback" role={busy ? 'status' : undefined}>
          {busy ? '正在保存订阅并获取节点，请稍候。关闭窗口不会取消请求，请勿重复添加。' : '保存订阅地址不代表已有可用节点；添加后请查看订阅获取状态。'}
        </p>
        <div className="modal-actions">
          <Button tone="ghost" size="sm" onClick={onClose}>{busy ? '关闭窗口' : '取消'}</Button>
          <Button
            tone="primary"
            size="sm"
            icon={<Plus size={ICON.xs} />}
            loading={busy}
            disabled={!url.trim() || busy}
            onClick={submit}
          >
            添加
          </Button>
        </div>
      </div>
    </div>
  )
}

export interface SubsCardProps {
  subs: SubStatus[]
  loadError?: string
  onRetry?: () => Promise<unknown>
  refreshStatus?: SubscriptionRefreshStatus
  pendingActions: ReadonlySet<string>
  onAddSub: (url: string) => Promise<boolean>
  onDeleteSub: (url: string) => void
  onRefreshSubs: () => void
  onToggleNodeDisabled: (sub: string, name: string, disabled: boolean) => Promise<boolean>
  /** 保存定时刷新设置；返回是否保存成功 */
  onSaveSchedule: (request: ScheduledRefreshRequest) => Promise<boolean>
  isInitializing: boolean
}

export function SubsCard({ subs, loadError, onRetry, refreshStatus, pendingActions, onAddSub, onDeleteSub, onRefreshSubs, onToggleNodeDisabled, onSaveSchedule, isInitializing }: SubsCardProps) {
  const [showAdd, setShowAdd] = useState(false)
  const [showSchedule, setShowSchedule] = useState(false)
  const [detailSub, setDetailSub] = useState<SubStatus | null>(null)
  const refreshing = pendingActions.has('refreshSubs')
  const deleting = pendingActions.has('deleteSub')

  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <Rss size={ICON.sm} className="section-icon" />
            <span>订阅管理</span>
            <button
              className="icon-button subtle"
              onClick={onRefreshSubs}
              disabled={subs.length === 0 || refreshing || isInitializing}
              aria-label="刷新订阅"
              title="刷新订阅"
            >
              <RefreshCw size={ICON.xs} className={refreshing ? 'spin' : undefined} />
            </button>
            <button
              className="icon-button subtle"
              onClick={() => setShowSchedule(true)}
              disabled={isInitializing}
              aria-label="定时刷新"
              title="定时刷新"
            >
              <Clock size={ICON.xs} />
            </button>
          </div>
          <Button
            tone="secondary"
            size="sm"
            icon={<Plus size={ICON.xs} />}
            disabled={isInitializing}
            onClick={() => setShowAdd(true)}
          >
            添加
          </Button>
        </div>
      }
    >
      <div className="list-stack">
        {deleting && <div className="list-row-meta" role="status">正在删除订阅并同步列表，请稍候…</div>}
        {loadError && <ResourceError label="订阅列表" error={loadError} hasData={subs.length > 0} onRetry={onRetry} />}
        {subs.length > 0 && (refreshing || refreshStatus?.phase === 'fetching') && (
          <div className="list-row" role="status">
            <div className="list-row-meta">正在获取订阅并检查配置，请稍候…</div>
          </div>
        )}
        {subs.length > 0 && !refreshing && refreshStatus && refreshStatus.phase !== 'fetching' && refreshStatus.outcome !== 'not_requested' && (
          <div className="list-row" role="status">
            <div className="list-row-meta subscription-feedback">
              最近一次获取：成功 {refreshStatus.report.successful_sources} 个订阅，失败 {refreshStatus.report.failed_sources} 个订阅；本次获取 {refreshStatus.report.fresh_nodes} 个节点，保留 {refreshStatus.report.cached_nodes} 个缓存节点。获取结果不代表配置已生效。
            </div>
          </div>
        )}
        {subs.length > 0 && refreshStatus?.phase === 'waiting' && (
          <div className="list-row" role="status">
            <div className="list-row-meta">
              后台等待重试{refreshStatus.retry_in_secs != null && refreshStatus.retry_in_secs > 0
                ? `（约 ${Math.ceil(refreshStatus.retry_in_secs / 60)} 分钟后）` : ''}，可手动刷新
            </div>
          </div>
        )}
        {subs.length === 0 
          ? !loadError && <div className="empty-block">暂无订阅</div>
          : subs.map((sub) => (
            <SubRow
              key={sub.url}
              sub={sub}
              onDelete={onDeleteSub}
              onShowNodes={setDetailSub}
              disabled={isInitializing || deleting}
            />
          ))}
      </div>
      <SubDetailModal
        sub={detailSub}
        onClose={() => setDetailSub(null)}
        onToggleNode={onToggleNodeDisabled}
      />
      <AddSubModal
        open={showAdd}
        loading={pendingActions.has('addSub')}
        onClose={() => setShowAdd(false)}
        onSubmit={onAddSub}
      />
      <ScheduleModal
        open={showSchedule}
        saving={pendingActions.has('scheduledRefresh')}
        onClose={() => setShowSchedule(false)}
        onSave={onSaveSchedule}
      />
    </SectionCard>
  )
}
