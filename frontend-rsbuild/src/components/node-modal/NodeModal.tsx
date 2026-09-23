import { useEffect, useId, useRef, useState } from 'react'
import type { Dispatch, SetStateAction } from 'react'
import { Plus, X } from 'lucide-react'
import { ICON } from '../../tokens'
import { useDialog } from '../../hooks/useDialog'
import { classNames, type NodeForm } from '../../utils'
import { LinkImportPane } from './LinkImportPane'
import { ManualPane } from './ManualPane'
import { VpsPane } from './VpsPane'
import type { BatchNodeResult, NodeRequest, NodeType, VpsDeployRequest } from '../../types/api'

type NodeModalMode = 'link' | 'manual' | 'vps'

export interface NodeModalProps {
  open: boolean
  nodeType: NodeType
  setNodeType: Dispatch<SetStateAction<NodeType>>
  form: NodeForm
  setForm: Dispatch<SetStateAction<NodeForm>>
  loading: boolean
  onClose: () => void
  onSubmit: () => Promise<boolean>
  onImport: (payloads: NodeRequest[]) => Promise<BatchNodeResult | undefined>
  onDeployVps: (req: VpsDeployRequest) => Promise<boolean>
  vpsSupported?: boolean
}

export function NodeModal({ open, nodeType, setNodeType, form, setForm, loading, onClose, onSubmit, onImport, onDeployVps, vpsSupported = true }: NodeModalProps) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [mode, setMode] = useState<NodeModalMode>('link')
  const [submitting, setSubmitting] = useState(false)
  const inFlight = useRef(false)
  const generation = useRef(0)
  const busy = loading || submitting

  // 关闭后重新打开时回到默认的链接导入模式
  useEffect(() => {
    if (!open) {
      setMode('link')
      generation.current++
    }
  }, [open])

  useEffect(() => {
    if (!vpsSupported && mode === 'vps') setMode('link')
  }, [vpsSupported, mode])

  if (!open) return null

  const submitManual = async () => {
    if (busy || inFlight.current) return
    inFlight.current = true
    setSubmitting(true)
    const submittedGeneration = generation.current
    try {
      if (await onSubmit() && submittedGeneration === generation.current) onClose()
    } finally {
      inFlight.current = false
      setSubmitting(false)
    }
  }

  const importLinks = async (payloads: NodeRequest[], closeOnSuccess: boolean) => {
    if (busy || inFlight.current) return
    inFlight.current = true
    setSubmitting(true)
    const submittedGeneration = generation.current
    try {
      const result = await onImport(payloads)
      if (submittedGeneration !== generation.current) return
      if (closeOnSuccess && result?.added.length === payloads.length && !result.failed.length) onClose()
      return result
    } finally {
      inFlight.current = false
      setSubmitting(false)
    }
  }

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={dialogRef}
        className="modal-card node-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal-title-row">
          <div className="modal-title-wrap">
            <Plus size={ICON.lg} className="icon-accent" />
            <h3 id={titleId}>添加节点</h3>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭节点对话框">
            <X size={ICON.md} />
          </button>
        </div>

        {submitting && <p className="node-submit-status" role="status">添加中，关闭窗口不会取消请求。</p>}
        <div className="connections-pills node-modal-tabs" role="group" aria-label="添加方式">
          <button
            type="button"
            disabled={submitting}
            className={classNames('connections-pill', mode === 'link' && 'active')}
            aria-pressed={mode === 'link'}
            onClick={() => setMode('link')}
          >
            粘贴链接
          </button>
          <button
            type="button"
            className={classNames('connections-pill', mode === 'manual' && 'active')}
            aria-pressed={mode === 'manual'}
            disabled={submitting}
            onClick={() => setMode('manual')}
          >
            手动填写
          </button>
          {vpsSupported ? (
            <button
              type="button"
              className={classNames('connections-pill', mode === 'vps' && 'active')}
              aria-pressed={mode === 'vps'}
              disabled={submitting}
              onClick={() => setMode('vps')}
            >
              VPS 部署
            </button>
          ) : null}
        </div>

        {mode === 'link' ? (
          <LinkImportPane onImport={importLinks} loading={busy} />
        ) : mode === 'vps' ? (
          <VpsPane onDeploy={onDeployVps} loading={busy} />
        ) : (
          <ManualPane
            nodeType={nodeType}
            setNodeType={setNodeType}
            form={form}
            setForm={setForm}
            loading={busy}
            onSubmit={submitManual}
          />
        )}
      </div>
    </div>
  )
}
