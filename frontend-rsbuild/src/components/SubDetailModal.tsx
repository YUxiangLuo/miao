import { useCallback, useEffect, useId, useRef, useState } from 'react'
import { Rss, X } from 'lucide-react'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { fetchJson } from '../hooks/request'
import { Button } from './ui'
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
  const [loading, setLoading] = useState(false)
  const loadingRef = useRef(false)
  // 操作跨窗口开关保留；同一订阅内的同名节点共享等待状态。
  const pendingRef = useRef(new Set<string>())
  const [pendingKeys, setPendingKeys] = useState<ReadonlySet<string>>(new Set())
  const loadGenerationRef = useRef(0)
  const activeUrlRef = useRef<string | null>(null)

  const load = useCallback(async (url: string) => {
    const generation = ++loadGenerationRef.current
    loadingRef.current = true
    setLoading(true)
    try {
      const payload = await fetchJson<ApiResponse<SubNodesInfo[]>>('/api/subs/nodes')
      if (generation !== loadGenerationRef.current) return
      if (payload.success && payload.data) {
        const group = payload.data.find((item) => item.url === url)
        if (!group) {
          setLoadError('当前节点列表中已找不到此订阅，请重新读取或关闭窗口后检查订阅列表。')
          return
        }
        setNodes(group.nodes)
        setStaleDisabled(group.stale_disabled ?? [])
        setLoadError('')
      } else {
        setLoadError(payload.message || '加载节点列表失败')
      }
    } catch (error) {
      if (generation === loadGenerationRef.current) setLoadError(error instanceof Error ? error.message : String(error))
    } finally {
      if (generation === loadGenerationRef.current) {
        loadingRef.current = false
        setLoading(false)
      }
    }
  }, [])

  // 打开时拉取；关闭时清空，下次打开重新加载
  useEffect(() => {
    activeUrlRef.current = sub?.url ?? null
    if (!sub) {
      loadGenerationRef.current += 1
      loadingRef.current = false
      setLoading(false)
      setNodes(null)
      setStaleDisabled([])
      setLoadError('')
      return
    }
    setNodes(null)
    setStaleDisabled([])
    setLoadError('')
    void load(sub.url)
  }, [sub, load])

  if (!sub) return null

  const toggle = async (name: string, disabled: boolean) => {
    const url = sub.url
    const key = JSON.stringify([url, name])
    if (pendingRef.current.has(key)) return
    pendingRef.current.add(key)
    setPendingKeys(new Set(pendingRef.current))
    try {
      const ok = await onToggleNode(url, name, disabled)
      if (ok && activeUrlRef.current === url) await load(url)
    } finally {
      pendingRef.current.delete(key)
      setPendingKeys(new Set(pendingRef.current))
    }
  }

  const isPending = (name: string) => pendingKeys.has(JSON.stringify([sub.url, name]))

  const disabledCount = nodes?.filter((node) => node.disabled).length ?? 0
  // 禁用节点排在最前面；组内保持订阅原始顺序（Array.sort 稳定）
  const sortedNodes = nodes === null ? null : [...nodes].sort((a, b) => Number(b.disabled) - Number(a.disabled))

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
          {loadError
            ? '节点列表尚未确认'
            : nodes === null
            ? '正在读取节点列表…'
            : `共 ${nodes.length} 个节点${disabledCount > 0 ? ` · 禁用 ${disabledCount}` : ''}`}
          {sub.state === 'failed' && sub.error ? ` · 上次获取失败：${sub.error}` : ''}
        </div>
        <div className="sub-detail-list list-stack">
          {loadError
            ? <div className="resource-error" role="alert">
                <strong>订阅节点读取失败</strong>
                <p>暂无法确认节点及禁用状态，不代表订阅没有节点。重新读取不会重新获取订阅或修改配置。</p>
                <Button tone="secondary" size="sm" loading={loading} onClick={() => {
                  if (!loadingRef.current) void load(sub.url)
                }}>重新读取节点</Button>
                <details><summary>错误详情</summary><p>{loadError}</p></details>
              </div>
            : sortedNodes === null
              ? null
              : sortedNodes.length === 0
                ? <div className="empty-block">暂无节点{sub.state === 'pending' ? '，等待首次获取' : ''}</div>
                : sortedNodes.map((node, index) => (
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
                      disabled={isPending(node.name)}
                      aria-busy={isPending(node.name) || undefined}
                      onClick={() => toggle(node.name, !node.disabled)}
                    >
                      <span className="toggle-switch-track">
                        <span className="toggle-switch-thumb" />
                      </span>
                    </button>
                  </div>
                ))}
        </div>
        {!loadError && staleDisabled.length > 0 && (
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
                  disabled={isPending(name)}
                  onClick={() => toggle(name, false)}
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
