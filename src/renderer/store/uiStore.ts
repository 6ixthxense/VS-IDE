import { create } from 'zustand'
import type { SysStats } from '@shared/types/ipc'
import { normalizeSysStats } from '../utils/sysStats'

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
  activeSidebarView: 'explorer' | 'search' | 'git'
  showSettingsModal: boolean
  showSidebar: boolean
  showQuickOpen: boolean
  toggleTerminal: () => void
  toggleCommandPalette: () => void
  toggleSysMonitor: () => void
  setSysStats: (stats: SysStats) => void
  setCursorPos: (pos: CursorPos) => void
  setPendingEditorTarget: (target: PendingEditorTarget | null) => void
  clearPendingEditorTarget: () => void
  setActiveSidebarView: (view: 'explorer' | 'search' | 'git') => void
  toggleSettingsModal: () => void
  toggleSidebar: () => void
  toggleQuickOpen: () => void
}

export const useUiStore = create<UiState>(set => ({
  showTerminal: true,
  showCommandPalette: false,
  showSysMonitor: true,
  sysStats: null,
  cursorPos: { line: 1, col: 1 },
  pendingEditorTarget: null,
  activeSidebarView: 'explorer',
  showSettingsModal: false,
  showSidebar: true,
  showQuickOpen: false,

  toggleTerminal: () => set(s => ({ showTerminal: !s.showTerminal })),
  toggleCommandPalette: () => set(s => ({ showCommandPalette: !s.showCommandPalette })),
  toggleSysMonitor: () => set(s => ({ showSysMonitor: !s.showSysMonitor })),
  setSysStats: (sysStats) => set({ sysStats: normalizeSysStats(sysStats) }),
  setCursorPos: (cursorPos) => set({ cursorPos }),
  setPendingEditorTarget: (pendingEditorTarget) => set({ pendingEditorTarget }),
  clearPendingEditorTarget: () => set({ pendingEditorTarget: null }),
  setActiveSidebarView: (activeSidebarView) => set({ activeSidebarView }),
  toggleSettingsModal: () => set(s => ({ showSettingsModal: !s.showSettingsModal })),
  toggleSidebar: () => set(s => ({ showSidebar: !s.showSidebar })),
  toggleQuickOpen: () => set(s => ({ showQuickOpen: !s.showQuickOpen })),
}))
