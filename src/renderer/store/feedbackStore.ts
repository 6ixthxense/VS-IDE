import { create } from 'zustand'

export type ToastTone = 'success' | 'error' | 'info' | 'warning'

export interface ToastItem {
  id: string
  title?: string
  message: string
  tone: ToastTone
  durationMs?: number
}

export interface ConfirmOptions {
  title: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
  tone?: ToastTone
}

interface ConfirmState extends ConfirmOptions {
  isOpen: boolean
}

interface FeedbackState {
  toasts: ToastItem[]
  confirmDialog: ConfirmState | null
  confirmResolver: ((result: boolean) => void) | null
  showToast: (toast: Omit<ToastItem, 'id'>) => string
  dismissToast: (id: string) => void
  confirm: (options: ConfirmOptions) => Promise<boolean>
  resolveConfirm: (result: boolean) => void
}

export const useFeedbackStore = create<FeedbackState>((set, get) => ({
  toasts: [],
  confirmDialog: null,
  confirmResolver: null,

  showToast: (toast) => {
    const id = `toast-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`

    set((state) => ({
      toasts: [...state.toasts, { ...toast, id }],
    }))

    return id
  },

  dismissToast: (id) => set((state) => ({
    toasts: state.toasts.filter((toast) => toast.id !== id),
  })),

  confirm: (options) => new Promise<boolean>((resolve) => {
    set({
      confirmDialog: {
        isOpen: true,
        title: options.title,
        message: options.message,
        confirmLabel: options.confirmLabel ?? 'Confirm',
        cancelLabel: options.cancelLabel ?? 'Cancel',
        tone: options.tone ?? 'info',
      },
      confirmResolver: resolve,
    })
  }),

  resolveConfirm: (result) => {
    const resolver = get().confirmResolver
    if (resolver) {
      resolver(result)
    }

    set({
      confirmDialog: null,
      confirmResolver: null,
    })
  },
}))

export function showSuccessToast(message: string, title = 'Done') {
  return useFeedbackStore.getState().showToast({ title, message, tone: 'success' })
}

export function showErrorToast(message: string, title = 'Something went wrong') {
  return useFeedbackStore.getState().showToast({ title, message, tone: 'error', durationMs: 5600 })
}

export function showInfoToast(message: string, title = 'Info') {
  return useFeedbackStore.getState().showToast({ title, message, tone: 'info' })
}

export function showWarningToast(message: string, title = 'Attention') {
  return useFeedbackStore.getState().showToast({ title, message, tone: 'warning' })
}

export function confirmAction(options: ConfirmOptions) {
  return useFeedbackStore.getState().confirm(options)
}
