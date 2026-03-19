import React, { useEffect, useRef } from 'react'
import { Terminal } from 'xterm'
import type { ITheme } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'
import { useEditorStore } from '../store/editorStore'
import { showInfoToast } from '../store/feedbackStore'
import { useTerminalStore } from '../store/terminalStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useConfigStore } from '../store/configStore'
import type { ThemeName } from '../config/appearance'
import { useUiStore } from '../store/uiStore'

interface TerminalSession {
  term: Terminal
  fit: FitAddon
  observer: ResizeObserver
  container: HTMLDivElement
}

function hexToRgba(hex: string, alpha: number) {
  const normalized = hex.replace('#', '')

  if (normalized.length !== 6) {
    return `rgba(38, 79, 120, ${alpha})`
  }

  const value = Number.parseInt(normalized, 16)
  const r = (value >> 16) & 255
  const g = (value >> 8) & 255
  const b = value & 255

  return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

function getTerminalTheme(theme: ThemeName, accent: string): ITheme {
  const selectionBackground = hexToRgba(accent, theme === 'light' ? 0.18 : 0.28)

  if (theme === 'light') {
    return {
      background: '#ffffff',
      foreground: '#1f2937',
      cursor: accent,
      cursorAccent: '#ffffff',
      selectionBackground,
      black: '#111827',
      red: '#dc2626',
      green: '#16a34a',
      yellow: '#ca8a04',
      blue: '#2563eb',
      magenta: '#9333ea',
      cyan: '#0891b2',
      white: '#6b7280',
      brightBlack: '#94a3b8',
      brightRed: '#ef4444',
      brightGreen: '#22c55e',
      brightYellow: '#eab308',
      brightBlue: '#3b82f6',
      brightMagenta: '#a855f7',
      brightCyan: '#06b6d4',
      brightWhite: '#0f172a',
    }
  }

  if (theme === 'amoled') {
    return {
      background: '#000000',
      foreground: '#f5f5f5',
      cursor: accent,
      cursorAccent: '#000000',
      selectionBackground,
      black: '#111111',
      red: '#ef4444',
      green: '#22c55e',
      yellow: '#f59e0b',
      blue: '#3b82f6',
      magenta: '#c084fc',
      cyan: '#22d3ee',
      white: '#e5e7eb',
      brightBlack: '#525252',
      brightRed: '#f87171',
      brightGreen: '#4ade80',
      brightYellow: '#fbbf24',
      brightBlue: '#60a5fa',
      brightMagenta: '#d8b4fe',
      brightCyan: '#67e8f9',
      brightWhite: '#ffffff',
    }
  }

  return {
    background: '#0d1117',
    foreground: '#e5e7eb',
    cursor: accent,
    cursorAccent: '#0d1117',
    selectionBackground,
    black: '#111827',
    red: '#f87171',
    green: '#4ade80',
    yellow: '#fbbf24',
    blue: '#60a5fa',
    magenta: '#c084fc',
    cyan: '#22d3ee',
    white: '#d1d5db',
    brightBlack: '#6b7280',
    brightRed: '#fca5a5',
    brightGreen: '#86efac',
    brightYellow: '#fde68a',
    brightBlue: '#93c5fd',
    brightMagenta: '#d8b4fe',
    brightCyan: '#67e8f9',
    brightWhite: '#f9fafb',
  }
}

export function TerminalPanel() {
  const termContainerRef = useRef<HTMLDivElement>(null)
  
  // Map of Terminal instances
  const termsRef = useRef<Map<string, TerminalSession>>(new Map())
  const { getActiveTab } = useEditorStore()
  const { rootPath } = useWorkspaceStore()
  const terminals = useTerminalStore((state) => state.terminals)
  const activeId = useTerminalStore((state) => state.activeId)
  const addTerminalSession = useTerminalStore((state) => state.addTerminal)
  const removeTerminalSession = useTerminalStore((state) => state.removeTerminal)
  const restoreTerminalSession = useTerminalStore((state) => state.restoreForWorkspace)
  const setActiveTerminal = useTerminalStore((state) => state.setActiveId)
  const updateTerminalCwd = useTerminalStore((state) => state.updateTerminalCwd)
  const theme = useConfigStore((s) => s.theme)
  const accent = useConfigStore((s) => s.accent)
  const fontFamily = useConfigStore((s) => s.fontFamily)
  const terminalFontSize = useConfigStore((s) => s.terminalFontSize)
  const showTerminal = useUiStore((s) => s.showTerminal)

  useEffect(() => {
    restoreTerminalSession(rootPath)
  }, [restoreTerminalSession, rootPath])

  useEffect(() => {
    // Only manage active terminal visibility
    termsRef.current.forEach((val, id) => {
      const el = document.getElementById(`terminal-${id}`)
      if (el) {
        const isActive = id === activeId
        el.style.display = isActive ? 'block' : 'none'
        if (isActive && showTerminal) {
          setTimeout(() => {
            val.fit.fit()
            val.term.focus()
          }, 100)
        }
      }
    })
  }, [activeId, showTerminal, terminals])

  useEffect(() => {
    const visibleTerminalIds = new Set(terminals.map((terminal) => terminal.id))

    termsRef.current.forEach(({ observer, term, container }, id) => {
      if (visibleTerminalIds.has(id)) return
      observer.disconnect()
      term.dispose()
      container.remove()
      termsRef.current.delete(id)
      window.electronAPI.closeTerminal(id)
    })

    const setupTerminal = (id: string, cwd: string) => {
      if (termsRef.current.has(id)) return
      
      const term = new Terminal({
        theme: getTerminalTheme(theme, accent),
        fontSize: terminalFontSize,
        lineHeight: 1.3,
        convertEol: true,
        cursorBlink: true,
        fontFamily,
      })
      const fit = new FitAddon()
      term.loadAddon(fit)
      
      const container = document.createElement('div')
      container.id = `terminal-${id}`
      container.className = 'terminal-instance'
      container.style.height = '100%'
      const isActive = id === activeId
      container.style.display = isActive ? 'block' : 'none'
      if (isActive) setTimeout(() => fit.fit(), 100)
      termContainerRef.current?.appendChild(container)
      
      term.open(container)
      fit.fit()
      term.writeln(`\x1b[90m── VS-Monitor Terminal [${id}] Ready ──\x1b[0m`)

      // Resize handling
      const observer = new ResizeObserver(() => {
        fit.fit()
        const dims = fit.proposeDimensions()
        if (dims) {
          window.electronAPI.resizeTerminal(id, dims.cols, dims.rows)
        }
      })
      observer.observe(container)

      term.onData(data => {
        window.electronAPI.sendTerminalInput(id, data)
      })

      termsRef.current.set(id, { term, fit, observer, container })
      window.electronAPI.createTerminal(id, cwd)
    }

    terminals.forEach((terminal) => setupTerminal(terminal.id, terminal.cwd || rootPath || ''))

    const cleanup = window.electronAPI.onTerminalOut(({ id, data }) => {
      const t = termsRef.current.get(id)
      if (t) t.term.write(data)
    })

    return () => {
      cleanup()
    }
  }, [accent, activeId, fontFamily, rootPath, terminalFontSize, terminals, theme])

  useEffect(() => {
    const nextTheme = getTerminalTheme(theme, accent)

    termsRef.current.forEach(({ term, fit }) => {
      term.options.theme = nextTheme
      term.options.fontFamily = fontFamily
      term.options.fontSize = terminalFontSize
      fit.fit()
    })
  }, [accent, fontFamily, terminalFontSize, theme])

  useEffect(() => {
    const sessions = termsRef.current

    return () => {
      sessions.forEach(({ observer, term, container }) => {
        observer.disconnect()
        term.dispose()
        container.remove()
      })
      sessions.clear()
    }
  }, [])

  const addTerminal = () => {
    const id = addTerminalSession(rootPath || '')
    setActiveTerminal(id)
  }

  const closeTerminal = (id: string, e: React.MouseEvent) => {
    e.stopPropagation()
    if (terminals.length === 1) return
    
    const t = termsRef.current.get(id)
    if (t) {
      t.observer.disconnect()
      t.term.dispose()
      t.container.remove()
      termsRef.current.delete(id)
    }
    window.electronAPI.closeTerminal(id)
    removeTerminalSession(id)
  }

  const handleRun = () => {
    const tab = getActiveTab()
    if (!tab) return
    
    const session = termsRef.current.get(activeId)
    if (session) {
      session.term.clear()
      session.term.focus()
      session.fit.fit()
    }
    
    window.electronAPI.runCode({
      terminalId: activeId,
      code: tab.content,
      language: tab.language === 'python' ? 'python' : 'javascript',
    })
  }

  const handleClear = () => termsRef.current.get(activeId)?.term.clear()

  const handleOpenActiveWorkspaceShell = () => {
    if (!rootPath) {
      showInfoToast('Open a workspace first, then terminal sessions will follow that folder.', 'No workspace yet')
      return
    }

    updateTerminalCwd(activeId, rootPath)
    window.electronAPI.setTerminalCwd(activeId, rootPath)
  }

  return (
    <div className="terminal-panel">
      <div className="terminal-header">
        <div className="terminal-tabs">
          {terminals.map(({ id }) => (
            <div 
              key={id} 
              className={`terminal-tab ${activeId === id ? 'active' : ''}`}
              onClick={() => setActiveTerminal(id)}
            >
              <span>{id === 'default' ? 'powershell' : 'term'}</span>
              <i className="fa-solid fa-xmark" onClick={(e) => closeTerminal(id, e)}></i>
            </div>
          ))}
          <button className="add-term-btn" onClick={addTerminal} title="New Terminal">
            <i className="fa-solid fa-plus"></i>
          </button>
        </div>
        <div className="terminal-actions">
          <button onClick={handleRun} title="Run current file (Ctrl+R)">▶ Run</button>
          <button onClick={handleOpenActiveWorkspaceShell} title="Jump terminal to workspace">⌂ Root</button>
          <button onClick={handleClear} title="Clear">⊘ Clear</button>
        </div>
      </div>
      <div ref={termContainerRef} className="terminal-body" />
    </div>
  )
}
