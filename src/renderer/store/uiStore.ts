import { create } from 'zustand'
import type { AppInfo, SysStats, UpdateStatusEvent } from '@shared/types/ipc'
import { normalizeSysStats } from '../utils/sysStats'
import { readWorkbenchUi, writeWorkbenchUi } from '../utils/workbenchPersistence'

export type SidebarView = 'explorer' | 'search' | 'git' | 'database' | 'problems' | 'tasks'

interface CursorPos {
  line: number
  col: number
}

interface PendingEditorTarget {
  filePath: string
  line: number
}

interface UiState {
  showTerminal: boolean
  showCommandPalette: boolean
  showSysMonitor: boolean
  sysStats: SysStats | null
  cursorPos: CursorPos
  pendingEditorTarget: PendingEditorTarget | null
  activeSidebarView: SidebarView
  showSettingsModal: boolean
  showDiagnosticsModal: boolean
  showSidebar: boolean
  showQuickOpen: boolean
  appInfo: AppInfo | null
  updateStatus: UpdateStatusEvent | null
  initWorkbenchUi: () => void
  toggleTerminal: () => void
  toggleCommandPalette: () => void
  toggleSysMonitor: () => void
  setSysStats: (stats: SysStats) => void
  setCursorPos: (pos: CursorPos) => void
  setPendingEditorTarget: (target: PendingEditorTarget | null) => void
  clearPendingEditorTarget: () => void
  setActiveSidebarView: (view: SidebarView) => void
  showSidebarView: (view: SidebarView) => void
  toggleSettingsModal: () => void
  toggleDiagnosticsModal: () => void
  toggleSidebar: () => void
  toggleQuickOpen: () => void
  setAppInfo: (info: AppInfo) => void
  setUpdateStatus: (status: UpdateStatusEvent | null) => void
}

function persistUiState(state: Pick<UiState, 'showTerminal' | 'showSysMonitor' | 'activeSidebarView' | 'showSidebar'>) {
  writeWorkbenchUi({
    showTerminal: state.showTerminal,
    showSysMonitor: state.showSysMonitor,
    activeSidebarView: state.activeSidebarView,
    showSidebar: state.showSidebar,
  })
}

export const useUiStore = create<UiState>((set) => ({
  showTerminal: true,
  showCommandPalette: false,
  showSysMonitor: true,
  sysStats: null,
  cursorPos: { line: 1, col: 1 },
  pendingEditorTarget: null,
  activeSidebarView: 'explorer',
  showSettingsModal: false,
  showDiagnosticsModal: false,
  showSidebar: true,
  showQuickOpen: false,
  appInfo: null,
  updateStatus: null,

  initWorkbenchUi: () => {
    const snapshot = readWorkbenchUi()
    if (!snapshot) return

    set({
      showTerminal: snapshot.showTerminal,
      showSysMonitor: snapshot.showSysMonitor,
      activeSidebarView: snapshot.activeSidebarView,
      showSidebar: snapshot.showSidebar,
    })
  },

  toggleTerminal: () => set((state) => {
    const nextState = { showTerminal: !state.showTerminal }
    persistUiState({ ...state, ...nextState })
    return nextState
  }),
  toggleCommandPalette: () => set(s => ({ showCommandPalette: !s.showCommandPalette })),
  toggleSysMonitor: () => set((state) => {
    const nextState = { showSysMonitor: !state.showSysMonitor }
    persistUiState({ ...state, ...nextState })
    return nextState
  }),
  setSysStats: (sysStats) => set({ sysStats: normalizeSysStats(sysStats) }),
  setCursorPos: (cursorPos) => set({ cursorPos }),
  setPendingEditorTarget: (pendingEditorTarget) => set({ pendingEditorTarget }),
  clearPendingEditorTarget: () => set({ pendingEditorTarget: null }),
  setActiveSidebarView: (activeSidebarView) => set((state) => {
    persistUiState({ ...state, activeSidebarView })
    return { activeSidebarView }
  }),
  showSidebarView: (activeSidebarView) => set((state) => {
    const nextState = { activeSidebarView, showSidebar: true }
    persistUiState({ ...state, ...nextState })
    return nextState
  }),
  toggleSettingsModal: () => set(s => ({ showSettingsModal: !s.showSettingsModal })),
  toggleDiagnosticsModal: () => set(s => ({ showDiagnosticsModal: !s.showDiagnosticsModal })),
  toggleSidebar: () => set((state) => {
    const nextState = { showSidebar: !state.showSidebar }
    persistUiState({ ...state, ...nextState })
    return nextState
  }),
  toggleQuickOpen: () => set(s => ({ showQuickOpen: !s.showQuickOpen })),
  setAppInfo: (appInfo) => set({ appInfo }),
  setUpdateStatus: (updateStatus) => set({ updateStatus }),
}))
