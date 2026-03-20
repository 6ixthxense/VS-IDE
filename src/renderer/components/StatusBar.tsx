import React from 'react'
import { useEditorStore } from '../store/editorStore'
import { useProblemsStore } from '../store/problemsStore'
import { useTasksStore } from '../store/tasksStore'
import { useUiStore } from '../store/uiStore'
import { countProblemSeverity, mergeProblemEntries } from '../utils/problems'

export function StatusBar() {
  const getActiveTab = useEditorStore((state) => state.getActiveTab)
  const cursorPos = useUiStore((state) => state.cursorPos)
  const appInfo = useUiStore((state) => state.appInfo)
  const updateStatus = useUiStore((state) => state.updateStatus)
  const toggleDiagnosticsModal = useUiStore((state) => state.toggleDiagnosticsModal)
  const showSidebarView = useUiStore((state) => state.showSidebarView)
  const problemEntries = useProblemsStore((state) =>
    mergeProblemEntries(state.result?.entries ?? [], state.liveEntries)
  )
  const runningTaskCount = useTasksStore((state) => state.runs.filter((run) => run.status === 'running').length)
  const activeTab = getActiveTab()
  const isDirty = activeTab && activeTab.content !== activeTab.savedContent
  const shouldShowUpdateStatus = updateStatus && ['checking', 'available', 'downloading', 'downloaded', 'error'].includes(updateStatus.state)
  const errorCount = countProblemSeverity(problemEntries, 'error')
  const warningCount = countProblemSeverity(problemEntries, 'warning')

  return (
    <div className="status-bar">
      <div className="status-left">
        <span className="status-indicator" />
        <span>{activeTab ? (isDirty ? `* ${activeTab.name}` : activeTab.name) : 'No file open'}</span>
      </div>
      <div className="status-right">
        {(errorCount > 0 || warningCount > 0) && (
          <button type="button" className="status-link" onClick={() => showSidebarView('problems')}>
            {errorCount}E {warningCount}W
          </button>
        )}
        {runningTaskCount > 0 && (
          <button type="button" className="status-link" onClick={() => showSidebarView('tasks')}>
            {runningTaskCount} task{runningTaskCount === 1 ? '' : 's'} running
          </button>
        )}
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
