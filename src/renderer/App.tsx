import React, { useEffect, useState, useCallback } from 'react'
import { Sidebar } from './components/Sidebar'
import { ActivityBar } from './components/ActivityBar'
import { EditorPane } from './components/EditorPane'
import { TerminalPanel } from './components/TerminalPanel'
import { StatusBar } from './components/StatusBar'
import { CommandPalette } from './features/command-palette/CommandPalette'
import { SettingsModal } from './components/SettingsModal'
import { SysMonitor } from './features/system-monitor/SysMonitor'
import { QuickOpen } from './features/quick-open/QuickOpen'
import { Resizer } from './components/Resizer'
import { useUiStore } from './store/uiStore'
import { useEditorStore } from './store/editorStore'
import { useWorkspaceStore } from './store/workspaceStore'
import { useSysStore } from './store/sysStore'
import { useConfigStore } from './store/configStore'

export default function App() {
  const sysStats = useUiStore((s) => s.sysStats)
  const showTerminal = useUiStore((s) => s.showTerminal)
  const showCommandPalette = useUiStore((s) => s.showCommandPalette)
  const showSysMonitor = useUiStore((s) => s.showSysMonitor)
  const showSidebar = useUiStore((s) => s.showSidebar)
  const showQuickOpen = useUiStore((s) => s.showQuickOpen)
  const theme = useConfigStore((s) => s.theme)

  const toggleTerminal = useUiStore((s) => s.toggleTerminal)
  const toggleCommandPalette = useUiStore((s) => s.toggleCommandPalette)
  const toggleQuickOpen = useUiStore((s) => s.toggleQuickOpen)
  const toggleSidebar = useUiStore((s) => s.toggleSidebar)
  const setSysStats = useUiStore((s) => s.setSysStats)
  const saveTab = useEditorStore((s) => s.saveTab)
  const getActiveTab = useEditorStore((s) => s.getActiveTab)

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
      useSysStore.getState().addStats(stats)
    })
    return cleanup
  }, [setSysStats])

  // Initialize workspace and theme on mount
  useEffect(() => {
    useWorkspaceStore.getState().init()
    document.documentElement.setAttribute('data-theme', theme)
  }, [])

  // Global keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key === 'P') {
        e.preventDefault()
        toggleCommandPalette()
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
  }, [toggleCommandPalette, toggleTerminal, saveTab, getActiveTab])

  return (
    <div className="app" data-theme={theme}>
      <div className="titlebar">
        <span className="titlebar-title">⌨️ VS-Monitor IDE</span>
        
        <div className="titlebar-stats">
          {sysStats && (
            <>
              <div className="stat-item" title="CPU Limit">
                <span style={{ color: '#0088ff' }}>CPU</span> {sysStats.cpu}%
              </div>
              <div className="stat-item" title="RAM Use">
                <span style={{ color: '#00cc66' }}>RAM</span> {sysStats.ram}%
              </div>
              {sysStats.disk && (
                <div className="stat-item" title="Disk Space">
                  <span style={{ color: '#ff9900' }}>DISK</span> {sysStats.disk.use}%
                </div>
              )}
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
          <EditorPane />
          {showTerminal && (
            <>
              <Resizer direction="vertical" onResize={handleTerminalResize} onResizeEnd={saveTerminalHeight} />
              <div style={{ height: terminalHeight, flexShrink: 0 }}>
                <TerminalPanel />
              </div>
            </>
          )}
        </div>

        {showSysMonitor && (
          <>
            <Resizer direction="horizontal" onResize={handleSysMonitorResize} onResizeEnd={saveSysMonitorWidth} />
            <aside className="right-panel" style={{ width: sysMonitorWidth }}>
              <SysMonitor stats={sysStats} />
            </aside>
          </>
        )}
      </div>

      <StatusBar />
      <CommandPalette />
      <QuickOpen />
      <SettingsModal />
    </div>
  )
}
