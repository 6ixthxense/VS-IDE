import React from 'react'
import { useEditorStore } from '../store/editorStore'
import { useUiStore } from '../store/uiStore'

export function StatusBar() {
  const getActiveTab = useEditorStore(s => s.getActiveTab)
  const cursorPos = useUiStore(s => s.cursorPos)
  const activeTab = getActiveTab()
  const isDirty = activeTab && activeTab.content !== activeTab.savedContent

  return (
    <div className="status-bar">
      <div className="status-left">
        <span className="status-indicator" />
        <span>{activeTab ? (isDirty ? `● ${activeTab.name}` : activeTab.name) : 'No file open'}</span>
      </div>
      <div className="status-right">
        {activeTab && (
          <>
            <span>{activeTab.language}</span>
            <span>Ln {cursorPos.line}, Col {cursorPos.col}</span>
          </>
        )}
        <span>VS-Monitor IDE</span>
      </div>
    </div>
  )
}
