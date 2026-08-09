import { useId } from 'react'
import { CircleAlert, X } from 'lucide-react'
import { useDialog } from '../hooks/useDialog.js'
import { Button } from './ui.jsx'

export function ConfirmModal({ open, title, message, onCancel, onConfirm }) {
  const titleId = useId()
  const dialogRef = useDialog(open, onCancel)

  if (!open) return null
  return (
    <div className="modal-overlay" onClick={onCancel}>
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
            <CircleAlert size={18} className="icon-warning" />
            <h3 id={titleId}>{title}</h3>
          </div>
          <button className="icon-button" onClick={onCancel} aria-label="关闭确认对话框">
            <X size={16} />
          </button>
        </div>
        <p className="modal-message">{message}</p>
        <div className="modal-actions">
          <Button tone="ghost" size="sm" onClick={onCancel}>取消</Button>
          <Button tone="danger" size="sm" data-autofocus onClick={onConfirm}>确认</Button>
        </div>
      </div>
    </div>
  )
}
