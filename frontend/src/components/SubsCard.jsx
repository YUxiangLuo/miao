import { memo } from 'react'
import { X, Check, CircleX, RefreshCw, Rss, Plus } from 'lucide-react'
import { Button, SectionCard } from './ui.jsx'
import { classNames, maskSubscription } from '../utils.js'

const SubRow = memo(function SubRow({ sub, onDelete }) {
  return (
    <div className="list-row">
      <div className={classNames('status-icon-badge', sub.success ? 'success' : 'error')}>
        {sub.success 
          ? <Check size={12} /> 
          : <CircleX size={12} />}
      </div>
      <div className="list-row-content">
        <div className="list-row-title">{maskSubscription(sub.url)}</div>
        <div className={classNames('list-row-meta', !sub.success && 'error')}>
          {sub.success 
            ? `${sub.node_count} 个节点` 
            : sub.error || '获取失败'}
        </div>
      </div>
      <button 
        className="icon-button subtle" 
        onClick={() => onDelete(sub.url)}
      >
        <X size={13} />
      </button>
    </div>
  )
})

export function SubsCard({ subs, newSubUrl, setNewSubUrl, loadingAction, onAddSub, onDeleteSub, onRefreshSubs, isInitializing }) {
  return (
    <SectionCard
      bodyClassName="panel-body-tight"
      header={
        <div className="section-header">
          <div className="section-title-wrap">
            <Rss size={14} className="section-icon" />
            <span>订阅管理</span>
          </div>
          <Button 
            tone="secondary" 
            size="sm" 
            icon={<RefreshCw size={12} />} 
            loading={loadingAction === 'refreshSubs'} 
            disabled={subs.length === 0 || loadingAction === 'refreshSubs' || isInitializing} 
            onClick={onRefreshSubs}
          >
            刷新
          </Button>
        </div>
      }
    >
      <div className="list-stack">
        {subs.length === 0 
          ? <div className="empty-block">暂无订阅</div> 
          : subs.map((sub) => (
            <SubRow key={sub.url} sub={sub} onDelete={onDeleteSub} />
          ))}
        <div className="subscription-add-row">
          <input 
            value={newSubUrl} 
            onChange={(event) => setNewSubUrl(event.target.value)} 
            onKeyDown={(event) => event.key === 'Enter' && onAddSub()} 
            placeholder="粘贴订阅链接..." 
          />
          <Button 
            tone="secondary" 
            size="sm" 
            icon={<Plus size={12} />} 
            loading={loadingAction === 'addSub'} 
            onClick={onAddSub}
          >
            添加
          </Button>
        </div>
      </div>
    </SectionCard>
  )
}
