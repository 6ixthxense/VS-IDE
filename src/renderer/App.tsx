import React, { Suspense, lazy, useEffect, useState, useCallback, useRef } from 'react'
import { Sidebar } from './components/Sidebar'
import { ActivityBar } from './components/ActivityBar'
import { StatusBar } from './components/StatusBar'
import { Resizer } from './components/Resizer'
import { ToastCenter } from './components/ToastCenter'
import { ConfirmDialog } from './components/ConfirmDialog'
import { useUiStore } from './store/uiStore'
import { useWorkspaceStore } from './store/workspaceStore'
import { useSysStore } from './store/sysStore'
import { useConfigStore } from './store/configStore'
import { useEditorStore } from './store/editorStore'
import { applyAppearanceConfig } from './config/appearance'
import { normalizeSysStats } from './utils/sysStats'

const EditorPane = lazy(() =>
  import('./components/EditorPane').then((module) => ({ default: module.EditorPane }))
)
const TerminalPanel = lazy(() =>
  import('./components/TerminalPanel').then((module) => ({ default: module.TerminalPanel }))
)
const CommandPalette = lazy(() =>
  import('./features/command-palette/CommandPalette').then((module) => ({ default: module.CommandPalette }))
)
const SettingsModal = lazy(() =>
  import('./components/SettingsModal').then((module) => ({ default: module.SettingsModal }))
)
const SysMonitor = lazy(() =>
  import('./features/system-monitor/SysMonitor').then((module) => ({ default: module.SysMonitor }))
)
const QuickOpen = lazy(() =>
  import('./features/quick-open/QuickOpen').then((module) => ({ default: module.QuickOpen }))
)

function PanelLoading({ label }: { label: string }) {
  return <div className="loading">{label}</div>
}

export default function App() {
  const sysStats = useUiStore((s) => s.sysStats)
  const showTerminal = useUiStore((s) => s.showTerminal)
  const showCommandPalette = useUiStore((s) => s.showCommandPalette)
  const showSysMonitor = useUiStore((s) => s.showSysMonitor)
  const showSidebar = useUiStore((s) => s.showSidebar)
  const showQuickOpen = useUiStore((s) => s.showQuickOpen)
  const showSettingsModal = useUiStore((s) => s.showSettingsModal)
  const theme = useConfigStore((s) => s.theme)
  const accent = useConfigStore((s) => s.accent)
  const accentGradient = useConfigStore((s) => s.accentGradient)
  const fontSize = useConfigStore((s) => s.fontSize)
  const terminalFontSize = useConfigStore((s) => s.terminalFontSize)
  const fontFamily = useConfigStore((s) => s.fontFamily)
  const workspaceRefreshTimerRef = useRef<number | null>(null)

  const toggleTerminal = useUiStore((s) => s.toggleTerminal)
  const toggleCommandPalette = useUiStore((s) => s.toggleCommandPalette)
  const toggleQuickOpen = useUiStore((s) => s.toggleQuickOpen)
  const toggleSidebar = useUiStore((s) => s.toggleSidebar)
  const setSysStats = useUiStore((s) => s.setSysStats)
  const normalizedSysStats = sysStats ? normalizeSysStats(sysStats) : null

  // Panel sizes (persisted in localStorage)
  const [sidebarWidth, setSidebarWidth] = useState(() => parseInt(localStorage.getItem('sidebarWidth') || '230'))
  const [terminalHeight, setTerminalHeight] = useState(() => parseInt(localStorage.getItem('terminalHeight') || '220'))
  const [sysMonitorWidth, setSysMonitorWidth] = useState(() => parseInt(localStorage.getItem('sysMonitorWidth') || '260'))

  const handleSidebarResize = useCallback((delta: number) => {
    setSidebarWidth(prev => Math.max(150, Math.min(500, prev + delta)))
  }, [])

  const handleTerminalResize = useCallback((delta: number) => {
    setTerminalHeight(prev => Math.max(100, Math.min(600, prev - delta)))
  }, [])

  const handleSysMonitorResize = useCallback((delta: number) => {
    setSysMonitorWidth(prev => Math.max(180, Math.min(500, prev - delta)))
  }, [])

  // Persist sizes
  const saveSidebarWidth = useCallback(() => localStorage.setItem('sidebarWidth', sidebarWidth.toString()), [sidebarWidth])
  const saveTerminalHeight = useCallback(() => localStorage.setItem('terminalHeight', terminalHeight.toString()), [terminalHeight])
  const saveSysMonitorWidth = useCallback(() => localStorage.setItem('sysMonitorWidth', sysMonitorWidth.toString()), [sysMonitorWidth])

  // Subscribe to IPC events
  useEffect(() => {
    const cleanup = window.electronAPI.onSysStats(stats => {
      setSysStats(stats)
      useSysStore.getState().addStats(normalizeSysStats(stats))
    })
    return cleanup
  }, [setSysStats])

  useEffect(() => {
    const cleanup = window.electronAPI.onWorkspaceChanged(() => {
      const { rootPath, refreshTree } = useWorkspaceStore.getState()
      if (!rootPath) return

      if (workspaceRefreshTimerRef.current) {
        window.clearTimeout(workspaceRefreshTimerRef.current)
      }

      workspaceRefreshTimerRef.current = window.setTimeout(() => {
        workspaceRefreshTimerRef.current = null
        void refreshTree()
      }, 180)
    })

    return () => {
      if (workspaceRefreshTimerRef.current) {
        window.clearTimeout(workspaceRefreshTimerRef.current)
      }
      cleanup()
    }
  }, [])

  // Initialize workspace and theme on mount
  useEffect(() => {
    useWorkspaceStore.getState().init()
  }, [])

  useEffect(() => {
    applyAppearanceConfig({
      theme,
      accent,
      accentGradient,
      fontSize,
      terminalFontSize,
      fontFamily,
    })
  }, [accent, accentGradient, fontFamily, fontSize, terminalFontSize, theme])

  // Global keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'P') {
        e.preventDefault()
        toggleCommandPalette()
      }
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === 't') {
        e.preventDefault()
        useEditorStore.getState().reopenClosedTab()
      }
      if ((e.ctrlKey || e.metaKey) && e.key === '`') {
        e.preventDefault()
        toggleTerminal()
      }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 'p') {
        e.preventDefault()
        toggleQuickOpen()
      }
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === 'b') {
        e.preventDefault()
        toggleSidebar()
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 'r') {
        e.preventDefault()
        document.querySelector<HTMLButtonElement>('.terminal-actions button')?.click()
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [toggleCommandPalette, toggleQuickOpen, toggleSidebar, toggleTerminal])

  return (
    <div className="app" data-theme={theme}>
      <div className="titlebar">
        <span className="titlebar-title">⌨️ VS-Monitor IDE</span>
        
        <div className="titlebar-stats">
          {normalizedSysStats && (
            <>
              <div className="stat-item" title="CPU Usage">
                <span style={{ color: '#0088ff' }}>CPU</span> {normalizedSysStats.cpu}%
              </div>
              <div className="stat-item" title="RAM Use">
                <span style={{ color: '#00cc66' }}>RAM</span> {normalizedSysStats.ram}%
              </div>
              {normalizedSysStats.gpu && (
                <div className="stat-item" title={normalizedSysStats.gpuName}>
                  <span style={{ color: '#9c27b0' }}>GPU</span> {normalizedSysStats.gpu.load}%
                </div>
              )}
              {normalizedSysStats.disks.map((disk) => (
                <div
                  key={disk.id}
                  className="stat-item"
                  title={disk.filesystem && disk.filesystem !== disk.name ? `${disk.name} • ${disk.filesystem}` : disk.name}
                >
                  <span style={{ color: '#ff9900' }}>{disk.name}</span> {disk.use}%
                </div>
              ))}
            </>
          )}
        </div>

        <div className="titlebar-actions">
          <button onClick={toggleTerminal} title="Toggle Terminal (Ctrl+`)">⬛</button>
          <button onClick={toggleCommandPalette} title="Command Palette (Ctrl+Shift+P)">⌘</button>
        </div>
      </div>

      <div className="main-layout">
        <ActivityBar />
        
        {showSidebar && (
          <>
            <div className="sidebar-container" style={{ width: sidebarWidth }}>
              <Sidebar />
            </div>
            <Resizer direction="horizontal" onResize={handleSidebarResize} onResizeEnd={saveSidebarWidth} />
          </>
        )}

        <div className="center-column" style={{ flex: 1, minWidth: 0 }}>
          <Suspense fallback={<PanelLoading label="Loading editor..." />}>
            <EditorPane />
          </Suspense>
          {showTerminal && (
            <>
              <Resizer direction="vertical" onResize={handleTerminalResize} onResizeEnd={saveTerminalHeight} />
              <div style={{ height: terminalHeight, flexShrink: 0 }}>
                <Suspense fallback={<PanelLoading label="Loading terminal..." />}>
                  <TerminalPanel />
                </Suspense>
              </div>
            </>
          )}
        </div>

        {showSysMonitor && (
          <>
            <Resizer direction="horizontal" onResize={handleSysMonitorResize} onResizeEnd={saveSysMonitorWidth} />
            <aside className="right-panel" style={{ width: sysMonitorWidth }}>
                <Suspense fallback={<PanelLoading label="Loading system monitor..." />}>
                  <SysMonitor stats={normalizedSysStats} />
                </Suspense>
              </aside>
            </>
        )}
      </div>

      <StatusBar />
      {showCommandPalette && (
        <Suspense fallback={null}>
          <CommandPalette />
        </Suspense>
      )}
      {showQuickOpen && (
        <Suspense fallback={null}>
          <QuickOpen />
        </Suspense>
      )}
      {showSettingsModal && (
        <Suspense fallback={null}>
          <SettingsModal />
        </Suspense>
      )}
      <ToastCenter />
      <ConfirmDialog />
    </div>
  )
}
