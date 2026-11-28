import { For, Show, createEffect, createMemo, createSignal, createUniqueId } from 'solid-js'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { fetchJson } from '../hooks/request'
import { usePresence } from '../motion'
import { Button } from './ui'
import { maskSubscription, protocolTone } from '../utils'
import type { ApiResponse, SubNodeInfo, SubNodesInfo, SubStatus } from '../types/api'
import { RssIcon, XIcon } from './icons'

export interface SubDetailModalProps {
  sub: SubStatus | null
  onClose: () => void
  onToggleNode: (sub: string, name: string, disabled: boolean) => Promise<boolean>
}

export function SubDetailModal(props: SubDetailModalProps) {
  const titleId = createUniqueId()
  const [panelEl, setPanelEl] = createSignal<HTMLElement | null>(null)
  const [lastSub, setLastSub] = createSignal<SubStatus | null>(null)
  const presence = usePresence({
    open: () => props.sub !== null,
    element: panelEl,
    exitDurationToken: 'fast',
  })
  const dialogRef = useDialog(() => props.sub !== null, () => props.onClose())
  const [nodes, setNodes] = createSignal<SubNodeInfo[] | null>(null)
  const [staleDisabled, setStaleDisabled] = createSignal<string[]>([])
  const [loadError, setLoadError] = createSignal('')
  const [loading, setLoading] = createSignal(false)
  let loadingFlag = false
  const pendingSet = new Set<string>()
  const [pendingKeys, setPendingKeys] = createSignal<ReadonlySet<string>>(new Set())
  let loadGeneration = 0
  let activeUrl: string | null = null

  const load = async (url: string) => {
    const generation = ++loadGeneration
    loadingFlag = true
    setLoading(true)
    try {
      const payload = await fetchJson<ApiResponse<SubNodesInfo[]>>('/api/subs/nodes')
      if (generation !== loadGeneration) return
      if (payload.success && payload.data) {
        const group = payload.data.find((item) => item.url === url)
        if (!group) {
          setLoadError('已找不到此订阅，请重新读取或返回订阅列表确认。')
          return
        }
        setNodes(group.nodes)
        setStaleDisabled(group.stale_disabled ?? [])
        setLoadError('')
      } else {
        setLoadError(payload.message || '加载节点列表失败')
      }
    } catch (error) {
      if (generation === loadGeneration) setLoadError(error instanceof Error ? error.message : String(error))
    } finally {
      if (generation === loadGeneration) {
        loadingFlag = false
        setLoading(false)
      }
    }
  }

  const subSnapMemo = createMemo(() => props.sub)
  createEffect(
    () => subSnapMemo(),
    (current) => {
      if (current) setLastSub(current)
    },
  )

  const subUrlMemo = createMemo(() => props.sub?.url ?? null)
  createEffect(
    () => subUrlMemo(),
    (url) => {
      activeUrl = url
      if (!url) {
        loadGeneration++
        loadingFlag = false
        setLoading(false)
        setNodes(null)
        setStaleDisabled([])
        setLoadError('')
        return
      }
      setNodes(null)
      setStaleDisabled([])
      setLoadError('')
      void load(url)
    },
  )

  const toggle = async (url: string, name: string, disabled: boolean) => {
    const key = JSON.stringify([url, name])
    if (pendingSet.has(key)) return
    pendingSet.add(key)
    setPendingKeys(new Set(pendingSet))
    try {
      const ok = await props.onToggleNode(url, name, disabled)
      if (ok && activeUrl === url) await load(url)
    } finally {
      pendingSet.delete(key)
      setPendingKeys(new Set(pendingSet))
    }
  }

  const isPending = (url: string, name: string) => pendingKeys().has(JSON.stringify([url, name]))
  const disabledCount = createMemo(() => nodes()?.filter((node) => node.disabled).length ?? 0)
  const sortedNodes = createMemo(() => {
    const list = nodes()
    return list === null ? null : [...list].sort((a, b) => Number(b.disabled) - Number(a.disabled))
  })

  const displaySub = createMemo(() => props.sub ?? lastSub())
  const sub = () => displaySub()!
  const isMounted = createMemo(() => presence.mounted() && Boolean(displaySub()))

  return (
    <Show when={isMounted()}>
      <div
        class="modal-overlay drawer-overlay"
        data-motion="scrim"
        data-motion-state={presence.motionState()}
        onClick={props.onClose}
      >
        <div
          ref={(el) => {
            setPanelEl(el)
            dialogRef(el)
          }}
          class="modal-card drawer-panel sub-detail-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          tabindex={-1}
          inert={props.sub === null ? true : undefined}
          data-motion="slide-edge"
          data-side="right"
          data-motion-state={presence.motionState()}
          onClick={(event) => event.stopPropagation()}
        >
          <div class="modal-title-row">
            <div class="modal-title-wrap">
              <RssIcon size={ICON.lg} class="icon-accent" />
              <h3 id={titleId} title={sub().url}>{maskSubscription(sub().url)}</h3>
            </div>
            <button type="button" class="icon-button" onClick={props.onClose} aria-label="关闭订阅详情">
              <XIcon size={ICON.md} />
            </button>
          </div>
          <div class="sub-detail-meta">
            {loadError()
              ? '节点列表尚未确认'
              : nodes() === null
                ? '正在读取节点列表…'
                : `共 ${nodes()!.length} 个节点${disabledCount() > 0 ? ` · 禁用 ${disabledCount()}` : ''}`}
            {sub().state === 'failed' && sub().error ? ` · 上次获取失败：${sub().error}` : ''}
          </div>
          <div class="sub-detail-list list-stack">
            <Show
              when={!loadError()}
              fallback={
                <div class="resource-error" role="alert">
                  <strong>订阅节点读取失败</strong>
                  <p>节点状态未知，不代表订阅没有节点。重试仅读取列表，不刷新订阅或修改配置。</p>
                  <Button
                    tone="secondary"
                    size="sm"
                    loading={loading()}
                    onClick={() => {
                      if (!loadingFlag) void load(sub().url)
                    }}
                  >
                    重新读取节点
                  </Button>
                  <details>
                    <summary>错误详情</summary>
                    <p>{loadError()}</p>
                  </details>
                </div>
              }
            >
              <Show when={sortedNodes()}>
                {(list) => (
                  <Show
                    when={list().length > 0}
                    fallback={
                      <div class="empty-block">
                        暂无节点{sub().state === 'pending' ? '，等待首次获取' : ''}
                      </div>
                    }
                  >
                    <For each={list()} keyed={(node) => `${node.name}:${node.server}:${node.server_port}`}>
                      {(node) => (
                        <div class={['list-row', 'sub-node-row', { disabled: node().disabled }]}>
                          <div class="list-row-content">
                            <div class="list-row-title" title={node().name}>{node().name}</div>
                            <div class="list-row-meta num">{node().server}:{node().server_port}</div>
                          </div>
                          <span class={['badge', protocolTone(node().node_type)]}>{node().node_type}</span>
                          <button
                            type="button"
                            role="switch"
                            aria-checked={!node().disabled ? 'true' : 'false'}
                            aria-label={`${node().disabled ? '启用' : '禁用'}节点 ${node().name}`}
                            title={node().disabled ? '启用该节点' : '禁用该节点（从生成的配置中移除）'}
                            class={['toggle-switch', { on: !node().disabled }]}
                            disabled={isPending(sub().url, node().name)}
                            aria-busy={isPending(sub().url, node().name) ? 'true' : undefined}
                            onClick={() => { void toggle(sub().url, node().name, !node().disabled) }}
                          >
                            <span class="toggle-switch-track">
                              <span class="toggle-switch-thumb" />
                            </span>
                          </button>
                        </div>
                      )}
                    </For>
                  </Show>
                )}
              </Show>
            </Show>
          </div>
          <Show when={!loadError() && staleDisabled().length > 0}>
            <div class="sub-stale-block">
              <div class="sub-detail-meta">已失效的禁用（订阅刷新后节点已改名/消失）：</div>
              <For each={staleDisabled()}>
                {(name) => (
                  <div class="list-row sub-stale-row">
                    <div class="list-row-content">
                      <div class="list-row-meta" title={name}>{name}</div>
                    </div>
                    <button
                      type="button"
                      class="icon-button subtle"
                      aria-label={`移除失效禁用 ${name}`}
                      title="移除该失效禁用条目"
                      disabled={isPending(sub().url, name)}
                      onClick={() => { void toggle(sub().url, name, false) }}
                    >
                      <XIcon size={ICON.xs} />
                    </button>
                  </div>
                )}
              </For>
            </div>
          </Show>
          <div class="sub-detail-foot">
            禁用后节点及引用它的规则不再生效；同名节点一起禁用。
          </div>
        </div>
      </div>
    </Show>
  )
}
