import React, { useMemo } from 'react'
import type { ProblemSource } from '@shared/types/ipc'
import { useEditorStore } from '../../store/editorStore'
import { useProblemsStore } from '../../store/problemsStore'
import { useUiStore } from '../../store/uiStore'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { countProblemSeverity, getProblemsTimestampLabel, mergeProblemEntries } from '../../utils/problems'

function toRelativePath(rootPath: string | null, filePath: string) {
  if (!rootPath) return filePath

  const normalizedRoot = rootPath.replace(/\\/g, '/')
  const normalizedPath = filePath.replace(/\\/g, '/')

  return normalizedPath.startsWith(`${normalizedRoot}/`)
    ? normalizedPath.slice(normalizedRoot.length + 1)
    : normalizedPath
}

function formatSourceLabel(source: ProblemSource) {
  return source === 'typescript' ? 'TypeScript' : 'ESLint'
}

export function ProblemsPanel() {
  const rootPath = useWorkspaceStore((state) => state.rootPath)
  const openFile = useEditorStore((state) => state.openFile)
  const result = useProblemsStore((state) => state.result)
  const liveEntries = useProblemsStore((state) => state.liveEntries)
  const loading = useProblemsStore((state) => state.loading)
  const severityFilter = useProblemsStore((state) => state.severityFilter)
  const sourceFilter = useProblemsStore((state) => state.sourceFilter)
  const setSeverityFilter = useProblemsStore((state) => state.setSeverityFilter)
  const setSourceFilter = useProblemsStore((state) => state.setSourceFilter)
  const scan = useProblemsStore((state) => state.scan)
  const setPendingEditorTarget = useUiStore((state) => state.setPendingEditorTarget)
  const allEntries = useMemo(
    () => mergeProblemEntries(result?.entries ?? [], liveEntries),
    [liveEntries, result?.entries]
  )

  const filteredEntries = useMemo(() => {
    return allEntries.filter((entry) => {
      if (severityFilter !== 'all' && entry.severity !== severityFilter) {
        return false
      }

      if (sourceFilter !== 'all' && entry.source !== sourceFilter) {
        return false
      }

      return true
    })
  }, [allEntries, severityFilter, sourceFilter])

  const errorCount = countProblemSeverity(allEntries, 'error')
  const warningCount = countProblemSeverity(allEntries, 'warning')
  const hasLiveTypeScript = liveEntries.length > 0

  const openProblem = async (filePath: string, line: number) => {
    setPendingEditorTarget({ filePath, line })
    await openFile(filePath)
  }

  return (
    <div className="problems-panel">
      <div className="sidebar-header">
        <span className="sidebar-title">PROBLEMS</span>
        <div className="sidebar-actions">
          <button onClick={() => { void scan() }} title="Scan Problems" disabled={!rootPath || loading}>
            <i className={`fa-solid ${loading ? 'fa-spinner fa-spin' : 'fa-bolt'}`}></i>
          </button>
        </div>
      </div>

      {!rootPath ? (
        <div className="sidebar-empty">
          <div className="panel-empty-card">
            <span className="panel-empty-eyebrow">Problems</span>
            <strong>Open a workspace to scan diagnostics</strong>
            <p>Run TypeScript and ESLint checks, filter the results, and jump straight to the affected line.</p>
          </div>
        </div>
      ) : (
        <>
          <div className="problems-toolbar">
            <div className="problems-summary-grid">
              <div className="problems-summary-card">
                <span className="problems-summary-label">Errors</span>
                <strong>{errorCount}</strong>
              </div>
              <div className="problems-summary-card warning">
                <span className="problems-summary-label">Warnings</span>
                <strong>{warningCount}</strong>
              </div>
              <div className="problems-summary-card neutral">
                <span className="problems-summary-label">Last Scan</span>
                <strong>{getProblemsTimestampLabel(result, liveEntries)}</strong>
              </div>
            </div>

            <div className="problems-filter-row">
              <button
                type="button"
                className={`search-filter-chip ${severityFilter === 'all' ? 'active' : ''}`}
                onClick={() => setSeverityFilter('all')}
              >
                All
              </button>
              <button
                type="button"
                className={`search-filter-chip ${severityFilter === 'error' ? 'active' : ''}`}
                onClick={() => setSeverityFilter('error')}
              >
                Errors
              </button>
              <button
                type="button"
                className={`search-filter-chip ${severityFilter === 'warning' ? 'active' : ''}`}
                onClick={() => setSeverityFilter('warning')}
              >
                Warnings
              </button>
              <button
                type="button"
                className={`search-filter-chip ${sourceFilter === 'all' ? 'active' : ''}`}
                onClick={() => setSourceFilter('all')}
              >
                All Sources
              </button>
              <button
                type="button"
                className={`search-filter-chip ${sourceFilter === 'typescript' ? 'active' : ''}`}
                onClick={() => setSourceFilter('typescript')}
              >
                TypeScript
              </button>
              <button
                type="button"
                className={`search-filter-chip ${sourceFilter === 'eslint' ? 'active' : ''}`}
                onClick={() => setSourceFilter('eslint')}
              >
                ESLint
              </button>
            </div>

            {(result?.notes?.length || result?.error || hasLiveTypeScript) ? (
              <div className="problems-notes">
                {hasLiveTypeScript && (
                  <div className="problems-note">
                    <i className="fa-solid fa-wave-square"></i>
                    <span>TypeScript issues are updating live from the editor. ESLint entries still come from scans.</span>
                  </div>
                )}
                {result?.error && (
                  <div className="problems-note error">
                    <i className="fa-solid fa-triangle-exclamation"></i>
                    <span>{result.error}</span>
                  </div>
                )}
                {(result?.notes ?? []).map((note) => (
                  <div key={note} className="problems-note">
                    <i className="fa-solid fa-circle-info"></i>
                    <span>{note}</span>
                  </div>
                ))}
              </div>
            ) : null}
          </div>

          <div className="problems-results">
            {filteredEntries.length === 0 ? (
              <div className="search-empty">
                <i className={`fa-solid ${loading ? 'fa-spinner fa-spin' : 'fa-circle-check'}`}></i>
                <strong>{loading ? 'Scanning workspace' : 'No problems in this view'}</strong>
                <span>
                  {result
                    ? 'Try another filter, or run a fresh scan after editing files.'
                    : hasLiveTypeScript
                      ? 'Live TypeScript diagnostics are ready. Run a scan to add ESLint results too.'
                      : 'Run your first scan to collect TypeScript and ESLint issues.'}
                </span>
              </div>
            ) : (
              filteredEntries.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  className={`problem-item ${entry.severity}`}
                  onClick={() => { void openProblem(entry.filePath, entry.line) }}
                  title={`${entry.filePath}:${entry.line}:${entry.column}`}
                >
                  <div className="problem-head">
                    <span className={`problem-severity ${entry.severity}`}>
                      {entry.severity === 'error' ? 'Error' : 'Warning'}
                    </span>
                    <span className="problem-source">
                      {formatSourceLabel(entry.source)}
                      {entry.code ? ` • ${entry.code}` : ''}
                    </span>
                  </div>
                  <div className="problem-message">{entry.message}</div>
                  <div className="problem-meta">
                    <span>{toRelativePath(rootPath, entry.filePath)}</span>
                    <span>Ln {entry.line}, Col {entry.column}</span>
                  </div>
                </button>
              ))
            )}
          </div>
        </>
      )}
    </div>
  )
}
