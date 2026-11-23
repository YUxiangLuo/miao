import { Match, Show, Switch, createEffect, createMemo, createSignal, createUniqueId, type Setter } from 'solid-js'
import { ICON } from '../../tokens'
import { useDialog } from '../../hooks/useDialog'
import type { NodeForm } from '../../utils'
import { LinkImportPane } from './LinkImportPane'
import { ManualPane } from './ManualPane'
import { VpsPane } from './VpsPane'
import type { BatchNodeResult, NodeRequest, NodeType, VpsDeployRequest } from '../../types/api'
import { PlusIcon, XIcon } from '../icons'

type NodeModalMode = 'link' | 'manual' | 'vps'

export interface NodeModalProps {
  open: boolean
  initialMode?: NodeModalMode
  nodeType: NodeType
  setNodeType: Setter<NodeType>
  form: NodeForm
  setForm: Setter<NodeForm>
  loading: boolean
  onClose: () => void
  onSubmit: () => Promise<boolean>
  onImport: (payloads: NodeRequest[]) => Promise<BatchNodeResult | undefined>
  onDeployVps: (req: VpsDeployRequest) => Promise<boolean>
  vpsSupported?: boolean
  relayCandidates?: string[]
}

export function NodeModal(props: NodeModalProps) {
  const titleId = createUniqueId()
  const dialogRef = useDialog(() => props.open, () => props.onClose())
  const [mode, setMode] = createSignal<NodeModalMode>('link')
  const [submitting, setSubmitting] = createSignal(false)
  let inFlight = false
  let generation = 0
  const busy = () => props.loading || submitting()
  const vpsSupported = () => props.vpsSupported !== false

  const activeMode = createMemo<NodeModalMode>(() => (!vpsSupported() && mode() === 'vps' ? 'link' : mode()))

  createEffect(
    () => ({ open: props.open, initialMode: props.initialMode }),
    ({ open, initialMode }) => {
      if (!open) {
        setMode('link')
        generation++
      } else if (initialMode) {
        setMode(initialMode === 'vps' && !vpsSupported() ? 'link' : initialMode)
      }
    },
  )

  const submitManual = async () => {
    if (busy() || inFlight) return
    inFlight = true
    setSubmitting(true)
    const submittedGeneration = generation
    try {
      if (await props.onSubmit() && submittedGeneration === generation) props.onClose()
    } finally {
      inFlight = false
      setSubmitting(false)
    }
  }

  const importLinks = async (payloads: NodeRequest[], closeOnSuccess: boolean) => {
    if (busy() || inFlight) return
    inFlight = true
    setSubmitting(true)
    const submittedGeneration = generation
    try {
      const result = await props.onImport(payloads)
      if (submittedGeneration !== generation) return
      if (closeOnSuccess && result?.added.length === payloads.length && !result.failed.length) props.onClose()
      return result
    } finally {
      inFlight = false
      setSubmitting(false)
    }
  }

  return (
    <Show when={props.open}>
      <div class="modal-overlay drawer-overlay" onClick={props.onClose}>
        <div
          ref={dialogRef}
          class="modal-card drawer-panel node-modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          tabindex={-1}
          onClick={(event) => event.stopPropagation()}
        >
          <div class="modal-title-row">
            <div class="modal-title-wrap">
              <PlusIcon size={ICON.lg} class="icon-accent" />
              <h3 id={titleId}>添加节点</h3>
            </div>
            <button type="button" class="icon-button" onClick={props.onClose} aria-label="关闭节点对话框">
              <XIcon size={ICON.md} />
            </button>
          </div>

          <Show when={submitting()}>
            <p class="node-submit-status" role="status">添加中，关闭窗口不会取消请求。</p>
          </Show>

          <div class="connections-pills node-modal-tabs" role="group" aria-label="添加方式">
            <button
              type="button"
              disabled={submitting()}
              class={['connections-pill', { active: activeMode() === 'link' }]}
              aria-pressed={activeMode() === 'link' ? 'true' : 'false'}
              onClick={() => setMode('link')}
            >
              粘贴链接
            </button>
            <button
              type="button"
              class={['connections-pill', { active: activeMode() === 'manual' }]}
              aria-pressed={activeMode() === 'manual' ? 'true' : 'false'}
              disabled={submitting()}
              onClick={() => setMode('manual')}
            >
              手动配置
            </button>
            <Show when={vpsSupported()}>
              <button
                type="button"
                class={['connections-pill', { active: activeMode() === 'vps' }]}
                aria-pressed={activeMode() === 'vps' ? 'true' : 'false'}
                disabled={submitting()}
                onClick={() => setMode('vps')}
              >
                VPS 部署
              </button>
            </Show>
          </div>

          <Switch>
            <Match when={activeMode() === 'link'}>
              <LinkImportPane onImport={importLinks} loading={busy()} />
            </Match>
            <Match when={activeMode() === 'vps'}>
              <VpsPane onDeploy={props.onDeployVps} loading={busy()} />
            </Match>
            <Match when={activeMode() === 'manual'}>
              <ManualPane
                nodeType={props.nodeType}
                setNodeType={props.setNodeType}
                form={props.form}
                setForm={props.setForm}
                loading={busy()}
                onSubmit={submitManual}
                relayCandidates={props.relayCandidates}
              />
            </Match>
          </Switch>
        </div>
      </div>
    </Show>
  )
}
