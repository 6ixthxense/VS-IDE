import React, { useEffect, useRef, useState } from 'react'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { useEditorStore } from '../../store/editorStore'
import { useUiStore } from '../../store/uiStore'

interface Command {
  id: string
  label: string
  icon: string
  shortcut?: string
  action: () => void | Promise<void>
}

export function CommandPalette() {
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const { showCommandPalette, toggleCommandPalette, toggleTerminal, toggleSysMonitor } = useUiStore()
  const openFolder = useWorkspaceStore(s => s.openFolder)
  const saveTab = useEditorStore(s => s.saveTab)
  const getActiveTab = useEditorStore(s => s.getActiveTab)

  const commands: Command[] = [
    { id: 'open-folder', label: 'Open Folder', icon: '📂', shortcut: 'Ctrl+O', action: async () => { await openFolder(); toggleCommandPalette() } },
    { id: 'save-file', label: 'Save File', icon: '💾', shortcut: 'Ctrl+S', action: async () => { const tab = getActiveTab(); if (tab) await saveTab(tab.id); toggleCommandPalette() } },
    { id: 'toggle-terminal', label: 'Toggle Terminal', icon: '⬛', shortcut: 'Ctrl+`', action: () => { toggleTerminal(); toggleCommandPalette() } },
    { id: 'toggle-sysmonitor', label: 'Toggle System Monitor', icon: '📊', shortcut: 'Ctrl+M', action: () => { toggleSysMonitor(); toggleCommandPalette() } },
  ]

  const filtered = commands.filter(c =>
    c.label.toLowerCase().includes(query.toLowerCase())
  )

  useEffect(() => {
    if (showCommandPalette) {
      setQuery('')
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [showCommandPalette])

  if (!showCommandPalette) return null

  return (
    <div className="palette-overlay" onClick={toggleCommandPalette}>
      <div className="palette" onClick={e => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="palette-input"
          placeholder="Type a command..."
          value={query}
          onChange={e => setQuery(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Escape') toggleCommandPalette()
            if (e.key === 'Enter' && filtered[0]) filtered[0].action()
          }}
        />
        <div className="palette-list">
          {filtered.map(cmd => (
            <div key={cmd.id} className="palette-item" onClick={() => cmd.action()}>
              <span className="palette-icon">{cmd.icon}</span>
              <span className="palette-label">{cmd.label}</span>
              {cmd.shortcut && <kbd className="palette-shortcut">{cmd.shortcut}</kbd>}
            </div>
          ))}
          {filtered.length === 0 && (
            <div className="palette-empty">No commands found</div>
          )}
        </div>
      </div>
    </div>
  )
}
