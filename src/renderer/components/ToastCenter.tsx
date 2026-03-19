import React, { useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useFeedbackStore } from '../store/feedbackStore'

const TOAST_ICON: Record<string, string> = {
  success: 'fa-circle-check',
  error: 'fa-circle-xmark',
  info: 'fa-circle-info',
  warning: 'fa-triangle-exclamation',
}

export function ToastCenter() {
  const toasts = useFeedbackStore((state) => state.toasts)
  const dismissToast = useFeedbackStore((state) => state.dismissToast)

  useEffect(() => {
    if (toasts.length === 0) return

    const timers = toasts.map((toast) =>
      window.setTimeout(() => dismissToast(toast.id), toast.durationMs ?? 3600)
    )

    return () => {
      timers.forEach((timer) => window.clearTimeout(timer))
    }
  }, [dismissToast, toasts])

  if (toasts.length === 0) return null

  return createPortal(
    <div className="toast-stack" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast-item toast-${toast.tone}`}>
          <div className="toast-icon">
            <i className={`fa-solid ${TOAST_ICON[toast.tone] ?? TOAST_ICON.info}`}></i>
          </div>
          <div className="toast-copy">
            {toast.title && <strong>{toast.title}</strong>}
            <span>{toast.message}</span>
          </div>
          <button type="button" className="toast-close" onClick={() => dismissToast(toast.id)} aria-label="Dismiss notification">
            <i className="fa-solid fa-xmark"></i>
          </button>
        </div>
      ))}
    </div>,
    document.body
  )
}
