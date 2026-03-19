import React, { useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useFeedbackStore } from '../store/feedbackStore'

const TOAST_ICON: Record<string, string> = {
  success: 'fa-circle-check',
  error: 'fa-circle-xmark',
  info: 'fa-circle-info',
  warning: 'fa-triangle-exclamation',
}

function ToastCard({
  tone,
  id,
  title,
  message,
  details,
  onDismiss,
}: {
  tone: string
  id: string
  title?: string
  message: string
  details?: string
  onDismiss: (id: string) => void
}) {
  const [expanded, setExpanded] = React.useState(false)

  return (
    <div className={`toast-item toast-${tone}`}>
      <div className="toast-icon">
        <i className={`fa-solid ${TOAST_ICON[tone] ?? TOAST_ICON.info}`}></i>
      </div>
      <div className="toast-copy">
        {title && <strong>{title}</strong>}
        <span>{message}</span>
        {details && (
          <>
            <div className="toast-detail-actions">
              <button type="button" className="toast-detail-btn" onClick={() => setExpanded((value) => !value)}>
                {expanded ? 'Hide details' : 'Show details'}
              </button>
              <button type="button" className="toast-detail-btn" onClick={() => void navigator.clipboard.writeText(details)}>
                Copy
              </button>
            </div>
            {expanded && <pre className="toast-details">{details}</pre>}
          </>
        )}
      </div>
      <button type="button" className="toast-close" onClick={() => onDismiss(id)} aria-label="Dismiss notification">
        <i className="fa-solid fa-xmark"></i>
      </button>
    </div>
  )
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
        <ToastCard key={toast.id} {...toast} onDismiss={dismissToast} />
      ))}
    </div>,
    document.body
  )
}
