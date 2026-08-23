import { useCallback, useEffect, useId, useState } from 'react'
import { Rss, X } from 'lucide-react'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { classNames, maskSubscription, protocolTone } from '../utils'
import type { ApiResponse, SubNodeInfo, SubNodesInfo, SubStatus } from '../types/api'

export interface SubDetailModalProps {
  /** null = 关闭；非 null 时展示该订阅的详情与节点列表 */
  sub: SubStatus | null
  onClose: () => void
  /** 切换节点禁用状态；返回是否成功（成功时本组件重新拉取节点列表） */
  onToggleNode: (sub: string, name: string, disabled: boolean) => Promise<boolean>
}

// 订阅详情弹窗：订阅信息 + 节点列表 + 单节点禁用开关（易变层，后端热应用）
export function SubDetailModal({ sub, onClose, onToggleNode }: SubDetailModalProps) {
  const titleId = useId()
  const dialogRef = useDialog(sub !== null, onClose)
  const [nodes, setNodes] = useState<SubNodeInfo[] | null>(null)
  const [staleDisabled, setStaleDisabled] = useState<string[]>([])
  const [loadError, setLoadError] = useState('')
  // 正在切换的节点名：行级 busy，防止重复点击
  const [pendingNames, setPendingNames] = useState<ReadonlySet<string>>(new Set())

  const load = useCallback(async (url: string) => {
    try {
      const response = await fetch('/api/subs/nodes')
      const payload: ApiResponse<SubNodesInfo[]> = await response.json()
      if (payload.success && payload.data) {
        const group = payload.data.find((item) => item.url === url)
        setNodes(group?.nodes ?? [])
        setStaleDisabled(group?.stale_disabled ?? [])
        setLoadError('')
      } else {
        setLoadError(payload.message || '加载节点列表失败')
      }
    } catch {
      setLoadError('加载节点列表失败')
    }
  }, [])

  // 打开时拉取；关闭时清空，下次打开重新加载
  useEffect(() => {
    if (!sub) {
      setNodes(null)
      setStaleDisabled([])
      setLoadError('')
      setPendingNames(new Set())
      return
    }
    void load(sub.url)
  }, [sub, load])

  if (!sub) return null

  const toggle = async (name: string, disabled: boolean) => {
    setPendingNames((prev) => new Set(prev).add(name))
    const ok = await onToggleNode(sub.url, name, disabled)
    if (ok) await load(sub.url)
    setPendingNames((prev) => {
      const next = new Set(prev)
      next.delete(name)
      return next
    })
  }

  const disabledCount = nodes?.filter((node) => node.disabled).length ?? 0

  // 移除失配的禁用条目（走「启用」路径按名字删除，不要求节点仍存在）
  const clearStale = async (name: string) => {
    setPendingNames((prev) => new Set(prev).add(name))
    const ok = await onToggleNode(sub.url, name, false)
    if (ok) await load(sub.url)
    setPendingNames((prev) => {
      const next = new Set(prev)
      next.delete(name)
      return next
    })
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card sub-detail-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Rss size={ICON.lg} className="icon-accent" />
            <h3 id={titleId} title={sub.url}>{maskSubscription(sub.url)}</h3>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭订阅详情">
            <X size={ICON.md} />
          </button>
        </div>
        <div className="sub-detail-meta">
          {nodes === null
            ? '加载中…'
            : `共 ${nodes.length} 个节点${disabledCount > 0 ? ` · 禁用 ${disabledCount}` : ''}`}
          {sub.state === 'failed' && sub.error ? ` · 上次获取失败：${sub.error}` : ''}
        </div>
        <div className="sub-detail-list list-stack">
          {loadError
            ? <div className="empty-block">{loadError}</div>
            : nodes === null
              ? null
              : nodes.length === 0
                ? <div className="empty-block">暂无节点{sub.state === 'pending' ? '，等待首次获取' : ''}</div>
                : nodes.map((node, index) => (
                  <div key={`${node.name}-${index}`} className={classNames('list-row', 'sub-node-row', node.disabled && 'disabled')}>
                    <div className="list-row-content">
                      <div className="list-row-title" title={node.name}>{node.name}</div>
                      <div className="list-row-meta">{node.server}:{node.server_port}</div>
                    </div>
                    <span className={classNames('badge', protocolTone(node.node_type))}>{node.node_type}</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!node.disabled}
                      aria-label={`${node.disabled ? '启用' : '禁用'}节点 ${node.name}`}
                      title={node.disabled ? '启用该节点' : '禁用该节点（从生成的配置中移除）'}
                      className={classNames('toggle-switch', !node.disabled && 'on')}
                      disabled={pendingNames.has(node.name)}
                      aria-busy={pendingNames.has(node.name) || undefined}
                      onClick={() => toggle(node.name, !node.disabled)}
                    >
                      <span className="toggle-switch-track">
                        <span className="toggle-switch-thumb" />
                      </span>
                    </button>
                  </div>
                ))}
        </div>
        {staleDisabled.length > 0 && (
          <div className="sub-stale-block">
            <div className="sub-detail-meta">已失效的禁用（订阅刷新后节点已改名/消失）：</div>
            {staleDisabled.map((name) => (
              <div key={name} className="list-row sub-stale-row">
                <div className="list-row-content">
                  <div className="list-row-meta" title={name}>{name}</div>
                </div>
                <button
                  type="button"
                  className="icon-button subtle"
                  aria-label={`移除失效禁用 ${name}`}
                  title="移除该失效禁用条目"
                  disabled={pendingNames.has(name)}
                  onClick={() => clearStale(name)}
                >
                  <X size={ICON.xs} />
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="sub-detail-foot">
          禁用的节点不会出现在生成的配置中；同名节点会一起禁用。自定义规则若引用了被禁节点将被跳过。
        </div>
      </div>
    </div>
  )
}
