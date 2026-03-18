import React, { useState, useEffect, useRef } from 'react'
import { useUiStore } from '../../store/uiStore'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { useEditorStore } from '../../store/editorStore'

export function QuickOpen() {
  const show = useUiStore(s => s.showQuickOpen)
  const toggle = useUiStore(s => s.toggleQuickOpen)
  const { rootPath } = useWorkspaceStore()
  const openFile = useEditorStore(s => s.openFile)

  const [query, setQuery] = useState('')
  const [files, setFiles] = useState<string[]>([])
  const [filtered, setFiltered] = useState<string[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (show) {
      inputRef.current?.focus()
      setQuery('')
      loadFiles()
    }
  }, [show])

  const loadFiles = async () => {
    if (!rootPath) return
    const allFiles = await window.electronAPI.getAllFiles(rootPath)
    setFiles(allFiles)
    setFiltered(allFiles.slice(0, 50)) // Initial view
  }

  useEffect(() => {
    const q = query.toLowerCase()
    const result = files
      .filter(f => f.toLowerCase().includes(q))
      .sort((a, b) => {
        // Better matching: prioritize filename over path
        const aName = a.split(/[\\/]/).pop()?.toLowerCase() || ''
        const bName = b.split(/[\\/]/).pop()?.toLowerCase() || ''
        if (aName.startsWith(q) && !bName.startsWith(q)) return -1
        if (!aName.startsWith(q) && bName.startsWith(q)) return 1
        return a.length - b.length
      })
      .slice(0, 10) // Limit results for performance
    
    setFiltered(result)
    setSelectedIndex(0)
  }, [query, files])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') toggle()
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIndex(s => (s + 1) % filtered.length)
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIndex(s => (s - 1 + filtered.length) % filtered.length)
    }
    if (e.key === 'Enter') {
      if (filtered[selectedIndex]) {
        openFile(filtered[selectedIndex])
        toggle()
      }
    }
  }

  if (!show) return null

  return (
    <div className="modal-overlay" onClick={toggle}>
      <div className="command-palette" onClick={e => e.stopPropagation()}>
        <div className="palette-input-wrapper">
          <i className="fa-solid fa-search"></i>
          <input
            ref={inputRef}
            placeholder="Search files by name..."
            value={query}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
        </div>
        <div className="palette-list">
          {filtered.map((path, idx) => {
             const parts = path.split(/[\\/]/)
             const name = parts.pop()
             const dir = parts.join('/')
             return (
               <div 
                 key={path} 
                 className={`palette-item ${idx === selectedIndex ? 'selected' : ''}`}
                 onClick={() => { openFile(path); toggle(); }}
               >
                 <div className="palette-info">
                    <div className="palette-label">{name}</div>
                    <div className="palette-sub" style={{ fontSize: '10px', opacity: 0.5 }}>{path.replace(rootPath || '', '')}</div>
                 </div>
               </div>
             )
          })}
          {filtered.length === 0 && <div className="palette-empty">No files found</div>}
        </div>
      </div>
    </div>
  )
}
