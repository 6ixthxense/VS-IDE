import React, { useEffect, useRef } from 'react'
import { Terminal } from 'xterm'
import { FitAddon } from 'xterm-addon-fit'
import 'xterm/css/xterm.css'
import { useEditorStore } from '../store/editorStore'
import { useWorkspaceStore } from '../store/workspaceStore'

export function TerminalPanel() {
  const [terminals, setTerminals] = React.useState<string[]>(['default'])
  const [activeId, setActiveId] = React.useState('default')
  const termContainerRef = useRef<HTMLDivElement>(null)
  
  // Map of Terminal instances
  const termsRef = useRef<Map<string, { term: Terminal, fit: FitAddon }>>(new Map())
  const { getActiveTab } = useEditorStore()
  const { rootPath } = useWorkspaceStore()

  useEffect(() => {
    // Only manage active terminal visibility
    termsRef.current.forEach((val, id) => {
      const el = document.getElementById(`terminal-${id}`)
      if (el) {
        el.style.display = id === activeId ? 'block' : 'none'
        if (id === activeId) val.fit.fit()
      }
    })
  }, [activeId, terminals])

  useEffect(() => {
    const setupTerminal = (id: string) => {
      if (termsRef.current.has(id)) return
      
      const term = new Terminal({
        theme: {
          background: '#0d0d0d',
          foreground: '#e0e0e0',
          cursor: '#ffffff',
          selectionBackground: '#264f78',
        },
        fontSize: 12,
        lineHeight: 1.3,
        convertEol: true,
        cursorBlink: true,
        fontFamily: "'Cascadia Code', 'Fira Code', Consolas, monospace",
      })
      const fit = new FitAddon()
      term.loadAddon(fit)
      
      const container = document.createElement('div')
      container.id = `terminal-${id}`
      container.className = 'terminal-instance'
      container.style.height = '100%'
      container.style.display = id === activeId ? 'block' : 'none'
      termContainerRef.current?.appendChild(container)
      
      term.open(container)
      fit.fit()
      term.writeln(`\x1b[90m── VS-Monitor Terminal [${id}] Ready ──\x1b[0m`)
      
      termsRef.current.set(id, { term, fit })

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

      return () => {
        observer.disconnect()
        term.dispose()
        container.remove()
      }
    }

    terminals.forEach(setupTerminal)

    const cleanup = window.electronAPI.onTerminalOut(({ id, data }) => {
      const t = termsRef.current.get(id)
      if (t) t.term.write(data)
    })

    return () => {
      cleanup()
    }
  }, [terminals, activeId, rootPath]) // Re-run when terminal list changes or activeId changes for initial display

  const addTerminal = () => {
    const id = `term-${Date.now()}`
    setTerminals([...terminals, id])
    setActiveId(id)
    window.electronAPI.createTerminal(id, rootPath || '')
  }

  const closeTerminal = (id: string, e: React.MouseEvent) => {
    e.stopPropagation()
    if (terminals.length === 1) return
    
    setTerminals(terminals.filter(t => t !== id))
    if (activeId === id) {
      // Find the next active terminal, prioritizing the one before the closed one
      const currentIndex = terminals.indexOf(id);
      const newActiveIndex = currentIndex > 0 ? currentIndex - 1 : (terminals.length > 1 ? 0 : -1);
      if (newActiveIndex !== -1) {
        setActiveId(terminals.filter(t => t !== id)[newActiveIndex]);
      } else {
        setActiveId(''); // No terminals left, though we prevent closing the last one
      }
    }
    
    const t = termsRef.current.get(id)
    if (t) {
      t.term.dispose()
      termsRef.current.delete(id)
    }
    window.electronAPI.closeTerminal(id)
    document.getElementById(`terminal-${id}`)?.remove()
  }

  const handleRun = () => {
    const tab = getActiveTab()
    if (!tab) return
    termsRef.current.get(activeId)?.term.clear()
    window.electronAPI.runCode({
      code: tab.content,
      language: tab.language === 'python' ? 'python' : 'javascript',
    })
  }

  const handleClear = () => termsRef.current.get(activeId)?.term.clear()

  return (
    <div className="terminal-panel">
      <div className="terminal-header">
        <div className="terminal-tabs">
          {terminals.map(id => (
            <div 
              key={id} 
              className={`terminal-tab ${activeId === id ? 'active' : ''}`}
              onClick={() => setActiveId(id)}
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
          <button onClick={handleClear} title="Clear">⊘ Clear</button>
        </div>
      </div>
      <div ref={termContainerRef} className="terminal-body" />
    </div>
  )
}
