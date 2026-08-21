import { useEffect, useId, useState } from 'react'
import type { Dispatch, SetStateAction } from 'react'
import { Plus, X } from 'lucide-react'
import { ICON } from '../../tokens'
import { useDialog } from '../../hooks/useDialog'
import { classNames, type NodeForm } from '../../utils'
import { LinkImportPane } from './LinkImportPane'
import { ManualPane } from './ManualPane'
import { VpsPane } from './VpsPane'
import type { NodeRequest, NodeType, VpsDeployRequest } from '../../types/api'

type NodeModalMode = 'link' | 'manual' | 'vps'

export interface NodeModalProps {
  open: boolean
  nodeType: NodeType
  setNodeType: Dispatch<SetStateAction<NodeType>>
  form: NodeForm
  setForm: Dispatch<SetStateAction<NodeForm>>
  loading: boolean
  onClose: () => void
  onSubmit: () => void
  onImport: (payloads: NodeRequest[]) => Promise<void>
  onDeployVps: (req: VpsDeployRequest) => Promise<boolean>
  vpsSupported?: boolean
}

export function NodeModal({ open, nodeType, setNodeType, form, setForm, loading, onClose, onSubmit, onImport, onDeployVps, vpsSupported = true }: NodeModalProps) {
  const titleId = useId()
  const dialogRef = useDialog(open, onClose)
  const [mode, setMode] = useState<NodeModalMode>('link')

  // 关闭后重新打开时回到默认的链接导入模式
  useEffect(() => {
    if (!open) setMode('link')
  }, [open])

  useEffect(() => {
    if (!vpsSupported && mode === 'vps') setMode('link')
  }, [vpsSupported, mode])

  if (!open) return null

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

        <div className="connections-pills node-modal-tabs" role="group" aria-label="添加方式">
          <button
            type="button"
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
            onClick={() => setMode('manual')}
          >
            手动填写
          </button>
          {vpsSupported ? (
            <button
              type="button"
              className={classNames('connections-pill', mode === 'vps' && 'active')}
              aria-pressed={mode === 'vps'}
              onClick={() => setMode('vps')}
            >
              VPS 部署
            </button>
          ) : null}
        </div>

        {mode === 'link' ? (
          <LinkImportPane onImport={onImport} loading={loading} />
        ) : mode === 'vps' ? (
          <VpsPane onDeploy={onDeployVps} loading={loading} />
        ) : (
          <ManualPane
            nodeType={nodeType}
            setNodeType={setNodeType}
            form={form}
            setForm={setForm}
            loading={loading}
            onSubmit={onSubmit}
          />
        )}
      </div>
    </div>
  )
}
