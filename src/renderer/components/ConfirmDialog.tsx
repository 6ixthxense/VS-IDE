import React, { useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useFeedbackStore } from '../store/feedbackStore'

const CONFIRM_ICON: Record<string, string> = {
  success: 'fa-circle-check',
  error: 'fa-circle-xmark',
  info: 'fa-circle-info',
  warning: 'fa-triangle-exclamation',
}

export function ConfirmDialog() {
  const confirmDialog = useFeedbackStore((state) => state.confirmDialog)
  const resolveConfirm = useFeedbackStore((state) => state.resolveConfirm)

  useEffect(() => {
    if (!confirmDialog?.isOpen) return

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        resolveConfirm(false)
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [confirmDialog?.isOpen, resolveConfirm])

  if (!confirmDialog?.isOpen) return null

  return createPortal(
    <div className="modal-overlay" onClick={() => resolveConfirm(false)}>
      <div className={`modal confirm-modal confirm-${confirmDialog.tone ?? 'info'}`} onClick={(event) => event.stopPropagation()}>
        <div className="confirm-head">
          <div className="confirm-icon">
            <i className={`fa-solid ${CONFIRM_ICON[confirmDialog.tone ?? 'info'] ?? CONFIRM_ICON.info}`}></i>
          </div>
          <div>
            <h3>{confirmDialog.title}</h3>
            <p>{confirmDialog.message}</p>
          </div>
        </div>
        <div className="modal-actions">
          <button type="button" className="btn-cancel" onClick={() => resolveConfirm(false)}>
            {confirmDialog.cancelLabel ?? 'Cancel'}
          </button>
          <button type="button" className="btn-confirm" onClick={() => resolveConfirm(true)}>
            {confirmDialog.confirmLabel ?? 'Confirm'}
          </button>
        </div>
      </div>
    </div>,
    document.body
  )
}
