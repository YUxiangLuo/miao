import { Show, createUniqueId } from 'solid-js'
import { ICON } from '../tokens'
import { useDialog } from '../hooks/useDialog'
import { Button } from './ui'
import { CircleAlertIcon, XIcon } from './icons'

export interface ConfirmModalProps {
  open: boolean
  title: string
  message: string
  onCancel: () => void
  onConfirm: () => void
}

export function ConfirmModal(props: ConfirmModalProps) {
  const titleId = createUniqueId()
  const dialogRef = useDialog(() => props.open, () => props.onCancel())

  return (
    <Show when={props.open}>
      <div class="modal-overlay" onClick={props.onCancel}>
        <div
          ref={dialogRef}
          class="modal-card modal-confirm"
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          tabindex={-1}
          onClick={(event) => event.stopPropagation()}
        >
          <div class="modal-title-row">
            <div class="modal-title-wrap">
              <CircleAlertIcon size={ICON.lg} class="icon-warning" />
              <h3 id={titleId}>{props.title}</h3>
            </div>
            <button type="button" class="icon-button" onClick={props.onCancel} aria-label="关闭确认对话框">
              <XIcon size={ICON.md} />
            </button>
          </div>
          <p class="modal-message">{props.message}</p>
          <div class="modal-actions">
            <Button tone="ghost" size="sm" onClick={props.onCancel}>取消</Button>
            <Button tone="danger" size="sm" data-autofocus onClick={props.onConfirm}>确认</Button>
          </div>
        </div>
      </div>
    </Show>
  )
}
