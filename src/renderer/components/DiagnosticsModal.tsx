import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import type { DiagnosticsSnapshot } from '@shared/types/ipc'
import { useUiStore } from '../store/uiStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { showErrorToast, showSuccessToast } from '../store/feedbackStore'

function formatTimestamp(value: string) {
  if (!value) return 'Unavailable'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString()
}

export function DiagnosticsModal() {
  const showDiagnosticsModal = useUiStore((state) => state.showDiagnosticsModal)
  const toggleDiagnosticsModal = useUiStore((state) => state.toggleDiagnosticsModal)
  const appInfo = useUiStore((state) => state.appInfo)
  const updateStatus = useUiStore((state) => state.updateStatus)
  const rootPath = useWorkspaceStore((state) => state.rootPath)
  const [snapshot, setSnapshot] = useState<DiagnosticsSnapshot | null>(null)
  const [isLoading, setIsLoading] = useState(false)

  const loadSnapshot = useCallback(async () => {
    setIsLoading(true)
    try {
      const nextSnapshot = await window.electronAPI.getDiagnostics()
      setSnapshot(nextSnapshot)
    } catch (error) {
      showErrorToast((error as Error).message || 'Unable to load diagnostics.', 'Diagnostics unavailable')
    } finally {
      setIsLoading(false)
    }
  }, [])

  useEffect(() => {
    if (!showDiagnosticsModal) return

    void loadSnapshot()

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        toggleDiagnosticsModal()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [loadSnapshot, showDiagnosticsModal, toggleDiagnosticsModal])

  const copySnapshot = useCallback(async () => {
    const payload = {
      appInfo,
      updateStatus,
      workspace: rootPath,
      diagnostics: snapshot,
    }

    try {
      await navigator.clipboard.writeText(JSON.stringify(payload, null, 2))
      showSuccessToast('Diagnostics snapshot copied to the clipboard.', 'Copied')
    } catch (error) {
      showErrorToast((error as Error).message || 'Clipboard access failed.', 'Copy failed')
    }
  }, [appInfo, rootPath, snapshot, updateStatus])

  const clearLogs = useCallback(async () => {
    const result = await window.electronAPI.clearDiagnostics()
    if (!result.success) {
      showErrorToast(result.error || 'Unable to clear diagnostics logs.', 'Clear failed', result.details)
      return
    }

    showSuccessToast('Diagnostics logs were cleared.', 'Logs cleared')
    await loadSnapshot()
  }, [loadSnapshot])

  const openLogFolder = useCallback(async () => {
    const result = await window.electronAPI.openLogFolder()
    if (!result.success) {
      showErrorToast(result.error || 'Unable to open the diagnostics folder.', 'Open failed', result.details)
    }
  }, [])

  const summaryItems = useMemo(() => ([
    { label: 'Version', value: appInfo ? `v${appInfo.version}` : 'Unknown' },
    { label: 'Workspace', value: rootPath ?? 'No workspace open' },
    { label: 'Bridge', value: appInfo?.bridgeAvailable ? 'Connected' : 'Unavailable' },
    { label: 'Updates', value: appInfo?.updateConfigured ? `${appInfo.updateSource || 'Configured'} (${appInfo.updateChannel || 'latest'})` : 'Disabled' },
    { label: 'Last update status', value: updateStatus?.message ?? 'Idle' },
    { label: 'Log file', value: snapshot?.logFilePath || 'Unavailable' },
  ]), [appInfo, rootPath, snapshot?.logFilePath, updateStatus?.message])

  if (!showDiagnosticsModal) return null

  return createPortal(
    <div className="modal-overlay diagnostics-overlay" onClick={toggleDiagnosticsModal}>
      <div className="modal diagnostics-modal" onClick={(event) => event.stopPropagation()}>
        <div className="diagnostics-header">
          <div>
            <div className="settings-eyebrow">
              <i className="fa-solid fa-stethoscope"></i> Diagnostics
            </div>
            <h3>Inspect app health, logs, and update wiring in one place.</h3>
            <p>Useful when a file save, terminal launch, update check, or renderer process acts unexpectedly.</p>
          </div>
          <div className="diagnostics-chip">
            {snapshot ? `${snapshot.entries.length} recent entries` : 'Loading...'}
          </div>
        </div>

        <div className="diagnostics-grid">
          {summaryItems.map((item) => (
            <div key={item.label} className="diagnostics-summary-card">
              <span>{item.label}</span>
              <strong title={item.value}>{item.value}</strong>
            </div>
          ))}
        </div>

        <div className="diagnostics-toolbar">
          <button type="button" onClick={() => { void loadSnapshot() }} disabled={isLoading}>
            Refresh
          </button>
          <button type="button" onClick={() => { void copySnapshot() }} disabled={!snapshot}>
            Copy Snapshot
          </button>
          <button type="button" onClick={() => { void openLogFolder() }}>
            Open Logs Folder
          </button>
          <button type="button" onClick={() => { void clearLogs() }} disabled={!snapshot || snapshot.entries.length === 0}>
            Clear Logs
          </button>
        </div>

        <div className="diagnostics-log-list">
          {isLoading && <div className="diagnostics-empty">Loading diagnostics...</div>}
          {!isLoading && snapshot && snapshot.entries.length === 0 && (
            <div className="diagnostics-empty">No diagnostics entries yet. The app has been pretty quiet.</div>
          )}
          {!isLoading && snapshot?.entries.map((entry) => (
            <article key={entry.id} className={`diagnostics-entry diagnostics-${entry.level}`}>
              <div className="diagnostics-entry-row">
                <span className="diagnostics-entry-source">{entry.source}</span>
                <span className="diagnostics-entry-time">{formatTimestamp(entry.timestamp)}</span>
              </div>
              <strong>{entry.message}</strong>
              {entry.details && <pre>{entry.details}</pre>}
            </article>
          ))}
        </div>

        <div className="modal-actions">
          <button type="button" className="btn-confirm" onClick={toggleDiagnosticsModal}>
            Done
          </button>
        </div>
      </div>
    </div>,
    document.body
  )
}
