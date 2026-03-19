import React from 'react'
import { useEditorStore } from '../store/editorStore'
import { useUiStore } from '../store/uiStore'

export function StatusBar() {
  const getActiveTab = useEditorStore(s => s.getActiveTab)
  const cursorPos = useUiStore(s => s.cursorPos)
  const appInfo = useUiStore((s) => s.appInfo)
  const updateStatus = useUiStore((s) => s.updateStatus)
  const toggleDiagnosticsModal = useUiStore((s) => s.toggleDiagnosticsModal)
  const activeTab = getActiveTab()
  const isDirty = activeTab && activeTab.content !== activeTab.savedContent
  const shouldShowUpdateStatus = updateStatus && ['checking', 'available', 'downloading', 'downloaded', 'error'].includes(updateStatus.state)

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
            {activeTab.recoveryState && (
              <span>{activeTab.recoveryState === 'conflict' ? 'Conflict on disk' : 'Recovered draft'}</span>
            )}
            <span>Ln {cursorPos.line}, Col {cursorPos.col}</span>
          </>
        )}
        {shouldShowUpdateStatus && (
          <span>{updateStatus.message}</span>
        )}
        <button type="button" className="status-link" onClick={toggleDiagnosticsModal}>
          Diagnostics
        </button>
        <span>{appInfo ? `VS-Monitor IDE v${appInfo.version}` : 'VS-Monitor IDE'}</span>
      </div>
    </div>
  )
}
