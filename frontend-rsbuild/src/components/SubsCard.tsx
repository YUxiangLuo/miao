import { memo, useEffect, useId, useState } from 'react'
import { Check, CircleX, Clock, RefreshCw, Rss, Plus, Trash2, X } from 'lucide-react'
import { ICON } from '../tokens'
import { Button, SectionCard } from './ui'
import { useDialog } from '../hooks/useDialog'
import { classNames, maskSubscription } from '../utils'
import { SubDetailModal } from './SubDetailModal'
import { ScheduleModal } from './ScheduleModal'
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

  // 关闭后重新打开时回到空输入
  useEffect(() => {
    if (!open) setUrl('')
  }, [open])

  if (!open) return null

  const submit = async () => {
    const trimmed = url.trim()
    if (!trimmed) return
    if (await onSubmit(trimmed)) onClose()
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
            onChange={(event) => setUrl(event.target.value)}
            onKeyDown={(event) => event.key === 'Enter' && submit()}
            placeholder="粘贴订阅链接..."
            aria-label="订阅链接"
            data-autofocus
          />
        </div>
        <div className="modal-actions">
          <Button tone="ghost" size="sm" onClick={onClose}>取消</Button>
          <Button
            tone="primary"
            size="sm"
            icon={<Plus size={ICON.xs} />}
            loading={loading}
            disabled={!url.trim()}
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

export function SubsCard({ subs, refreshStatus, pendingActions, onAddSub, onDeleteSub, onRefreshSubs, onToggleNodeDisabled, onSaveSchedule, isInitializing }: SubsCardProps) {
  const [showAdd, setShowAdd] = useState(false)
  const [showSchedule, setShowSchedule] = useState(false)
  const [detailSub, setDetailSub] = useState<SubStatus | null>(null)
  const refreshing = pendingActions.has('refreshSubs')

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
          </div>
          <div className="section-actions">
            <Button
              tone="secondary"
              size="sm"
              icon={<Clock size={ICON.xs} />}
              disabled={isInitializing}
              onClick={() => setShowSchedule(true)}
            >
              定时刷新
            </Button>
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
        </div>
      }
    >
      <div className="list-stack">
        {subs.length > 0 && refreshStatus?.phase === 'waiting' && (
          <div className="list-row" role="status">
            <div className="list-row-meta">
              后台等待重试{refreshStatus.retry_in_secs != null && refreshStatus.retry_in_secs > 0
                ? `（约 ${Math.ceil(refreshStatus.retry_in_secs / 60)} 分钟后）` : ''}，可手动刷新
            </div>
          </div>
        )}
        {subs.length === 0 
          ? <div className="empty-block">暂无订阅</div> 
          : subs.map((sub) => (
            <SubRow
              key={sub.url}
              sub={sub}
              onDelete={onDeleteSub}
              onShowNodes={setDetailSub}
              disabled={isInitializing}
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
