import { memo, useEffect, useId, useState } from 'react'
import { Check, CircleX, RefreshCw, Rss, Plus, Trash2, X } from 'lucide-react'
import { ICON } from '../tokens'
import { Button, SectionCard } from './ui'
import { useDialog } from '../hooks/useDialog'
import { classNames, maskSubscription } from '../utils'
import { SubDetailModal } from './SubDetailModal'
import type { SubStatus } from '../types/api'

interface SubRowProps {
  sub: SubStatus
  onDelete: (url: string) => void
  onShowNodes: (sub: SubStatus) => void
  disabled: boolean
}

const SubRow = memo(function SubRow({ sub, onDelete, onShowNodes, disabled }: SubRowProps) {
  const state = sub.state || (sub.success ? 'ready' : 'failed')
  const pending = state === 'pending' || state === 'refreshing'
  // 获取成功的订阅节点数可点击：打开订阅详情弹窗（节点列表 + 禁用开关）
  const clickable = sub.success && !pending && sub.node_count > 0
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
        {clickable
          ? (
            <button
              type="button"
              className="list-row-meta meta-link"
              title="查看订阅节点"
              onClick={() => onShowNodes(sub)}
            >
              {sub.node_count} 个节点{sub.disabled_count > 0 ? ` · 禁用 ${sub.disabled_count}` : ''}
            </button>
          )
          : (
            <div
              className={classNames('list-row-meta', state === 'failed' && 'error')}
              title={state === 'failed' ? sub.error : undefined}
            >
              {state === 'pending'
                ? '等待首次获取'
                : state === 'refreshing'
                  ? sub.success ? `正在刷新，上次获取 ${sub.node_count} 个节点` : '正在获取订阅'
                  : sub.success
                    ? `${sub.node_count} 个节点`
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
  loadingAction: string
  onAddSub: (url: string) => Promise<boolean>
  onDeleteSub: (url: string) => void
  onRefreshSubs: () => void
  onToggleNodeDisabled: (sub: string, name: string, disabled: boolean) => Promise<boolean>
  isInitializing: boolean
}

export function SubsCard({ subs, loadingAction, onAddSub, onDeleteSub, onRefreshSubs, onToggleNodeDisabled, isInitializing }: SubsCardProps) {
  const [showAdd, setShowAdd] = useState(false)
  const [detailSub, setDetailSub] = useState<SubStatus | null>(null)
  const refreshing = loadingAction === 'refreshSubs'

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
        loading={loadingAction === 'addSub'}
        onClose={() => setShowAdd(false)}
        onSubmit={onAddSub}
      />
    </SectionCard>
  )
}
