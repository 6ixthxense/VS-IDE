import React, { useState, useCallback, useEffect, useMemo } from 'react'
import type { GitFileStatus } from '@shared/types/ipc'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { useEditorStore } from '../../store/editorStore'
import { confirmAction, showErrorToast, showSuccessToast } from '../../store/feedbackStore'

interface ChangeSelection {
  filePath: string
  status: GitFileStatus | string
  staged: boolean
}

function getStatusMeta(status: string, staged: boolean) {
  switch (status) {
    case 'modified':
      return { label: staged ? 'Staged edit' : 'Modified', short: 'M', color: '#f4c96b' }
    case 'added':
      return { label: staged ? 'Staged add' : 'Added', short: 'A', color: '#7dd3fc' }
    case 'untracked':
      return { label: 'Untracked', short: 'U', color: '#7dd3fc' }
    case 'deleted':
      return { label: staged ? 'Staged delete' : 'Deleted', short: 'D', color: '#f87171' }
    case 'renamed':
      return { label: 'Renamed', short: 'R', color: '#c084fc' }
    case 'staged':
      return { label: 'Staged', short: 'A', color: '#4ade80' }
    default:
      return { label: staged ? 'Staged' : 'Changed', short: '•', color: 'var(--txt)' }
  }
}

function getRelativePath(filePath: string, rootPath: string) {
  if (!filePath.startsWith(rootPath)) return filePath.replace(/\\/g, '/')
  return filePath.slice(rootPath.length).replace(/^[\\/]/, '').replace(/\\/g, '/')
}

function classifyDiffLine(line: string) {
  if (
    line.startsWith('diff --git') ||
    line.startsWith('index ') ||
    line.startsWith('--- ') ||
    line.startsWith('+++ ')
  ) {
    return 'git-diff-line header'
  }

  if (line.startsWith('@@')) return 'git-diff-line meta'
  if (line.startsWith('+')) return 'git-diff-line added'
  if (line.startsWith('-')) return 'git-diff-line removed'
  return 'git-diff-line'
}

export function GitPanel() {
  const { rootPath, gitStatus, refreshTree } = useWorkspaceStore()
  const openFile = useEditorStore((state) => state.openFile)
  const [commitMsg, setCommitMsg] = useState('')
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [selectedChange, setSelectedChange] = useState<ChangeSelection | null>(null)
  const [diffPreview, setDiffPreview] = useState('')
  const [isDiffLoading, setIsDiffLoading] = useState(false)

  const handleRefresh = useCallback(async () => {
    setIsRefreshing(true)
    await refreshTree()
    setIsRefreshing(false)
  }, [refreshTree])

  const handleAdd = async (filePath: string) => {
    const result = await window.electronAPI.gitAdd(rootPath!, [filePath])
    if (result.success) {
      await refreshTree()
      showSuccessToast('Change staged successfully.', 'Staged')
    } else {
      showErrorToast(result.error || 'Unable to stage this change.', 'Stage failed', result.details)
    }
  }

  const handleAddAll = async () => {
    const result = await window.electronAPI.gitAdd(rootPath!, ['.'])
    if (result.success) {
      await refreshTree()
      showSuccessToast('All current changes are staged.', 'Stage complete')
    } else {
      showErrorToast(result.error || 'Unable to stage all changes.', 'Stage failed', result.details)
    }
  }

  const handleUnstage = async (filePath: string) => {
    const result = await window.electronAPI.gitUnstage(rootPath!, [filePath])
    if (result.success) {
      await refreshTree()
      showSuccessToast('Change moved back to the working tree.', 'Unstaged')
    } else {
      showErrorToast(result.error || 'Unable to unstage this change.', 'Unstage failed', result.details)
    }
  }

  const handleDiscard = async (change: ChangeSelection) => {
    if (change.status === 'renamed') {
      showErrorToast('Discard for renamed files is not supported yet.', 'Discard unavailable')
      return
    }

    const confirmed = await confirmAction({
      title: 'Discard local changes?',
      message:
        change.status === 'untracked' || change.status === 'added'
          ? 'This removes the new file from the working tree.'
          : 'This restores the file to the last committed version and cannot be undone from the app.',
      confirmLabel: 'Discard',
      cancelLabel: 'Cancel',
      tone: 'warning',
    })

    if (!confirmed) return

    const result = await window.electronAPI.gitDiscard(rootPath!, change)
    if (result.success) {
      await refreshTree()
      showSuccessToast('Local changes were discarded.', 'Discard complete')
    } else {
      showErrorToast(result.error || 'Unable to discard this change.', 'Discard failed', result.details)
    }
  }

  const handleCommit = async () => {
    if (!commitMsg.trim()) return
    const result = await window.electronAPI.gitCommit(rootPath!, commitMsg)
    if (result.success) {
      setCommitMsg('')
      await refreshTree()
      showSuccessToast('Commit created successfully.', 'Commit complete')
    } else {
      showErrorToast(result.error || 'Commit failed.', 'Commit failed', result.details)
    }
  }

  const handlePush = async () => {
    const result = await window.electronAPI.gitPush(rootPath!)
    if (result.success) showSuccessToast('Remote updated successfully.', 'Push complete')
    else showErrorToast(result.error || 'Push failed.', 'Push failed', result.details)
  }

  const handlePull = async () => {
    const result = await window.electronAPI.gitPull(rootPath!)
    if (result.success) {
       showSuccessToast('Workspace is up to date.', 'Pull complete')
       await refreshTree()
    } else {
      showErrorToast(result.error || 'Pull failed.', 'Pull failed', result.details)
    }
  }

  const staged = useMemo(
    () => Object.entries(gitStatus)
      .filter(([_, status]) => status === 'staged')
      .sort(([a], [b]) => a.localeCompare(b)),
    [gitStatus]
  )
  const unstaged = useMemo(
    () => Object.entries(gitStatus)
      .filter(([_, status]) => status !== 'staged')
      .sort(([a], [b]) => a.localeCompare(b)),
    [gitStatus]
  )

  const allChanges = useMemo<ChangeSelection[]>(
    () => [
      ...unstaged.map(([filePath, status]) => ({ filePath, status, staged: false })),
      ...staged.map(([filePath, status]) => ({ filePath, status, staged: true })),
    ],
    [staged, unstaged]
  )

  useEffect(() => {
    if (!rootPath || allChanges.length === 0) {
      setSelectedChange(null)
      setDiffPreview('')
      return
    }

    const selectionStillExists = selectedChange && allChanges.some((change) =>
      change.filePath === selectedChange.filePath &&
      change.status === selectedChange.status &&
      change.staged === selectedChange.staged
    )

    if (!selectionStillExists) {
      setSelectedChange(allChanges[0])
    }
  }, [allChanges, rootPath, selectedChange])

  useEffect(() => {
    if (!rootPath || !selectedChange) {
      setDiffPreview('')
      setIsDiffLoading(false)
      return
    }

    let cancelled = false
    setIsDiffLoading(true)

    window.electronAPI
      .getGitDiff(rootPath, selectedChange)
      .then((diff) => {
        if (!cancelled) {
          setDiffPreview(diff)
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDiffPreview('Unable to load diff preview.')
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsDiffLoading(false)
        }
      })

    return () => {
      cancelled = true
    }
  }, [rootPath, selectedChange])

  if (!rootPath) {
    return (
      <div className="git-panel-empty">
        <div className="panel-empty-card">
          <span className="panel-empty-eyebrow">Source Control</span>
          <strong>Open a workspace to inspect changes</strong>
          <p>Commit history, staged files, and diff preview appear here once a folder is open.</p>
          <button className="open-folder-btn" onClick={() => useWorkspaceStore.getState().openFolder()}>
            <i className="fa-solid fa-folder-open"></i> Open Folder
          </button>
        </div>
      </div>
    )
  }

  const selectedMeta = selectedChange ? getStatusMeta(selectedChange.status, selectedChange.staged) : null
  const selectedName = selectedChange
    ? selectedChange.filePath.split(window.electronAPI.isWindows ? '\\' : '/').pop() || 'unknown'
    : ''
  const selectedRelativePath = selectedChange ? getRelativePath(selectedChange.filePath, rootPath) : ''
  const diffLines = diffPreview ? diffPreview.split('\n') : []
  const visibleDiffLines = diffLines.slice(0, 240)
  const isDiffTruncated = diffLines.length > visibleDiffLines.length

  return (
    <div className="git-panel">
      <div className="sidebar-header">
        <span className="sidebar-title">SOURCE CONTROL</span>
        <div className="sidebar-actions">
          <button onClick={handlePull} title="Pull"><i className="fa-solid fa-arrow-down"></i></button>
          <button onClick={handlePush} title="Push"><i className="fa-solid fa-arrow-up"></i></button>
          <button onClick={handleRefresh} className={isRefreshing ? 'fa-spin' : ''} title="Refresh"><i className="fa-solid fa-rotate"></i></button>
        </div>
      </div>

      <div className="git-commit-box">
        <textarea 
          placeholder="Commit message (Ctrl+Enter to commit)"
          value={commitMsg}
          onChange={e => setCommitMsg(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) handleCommit()
          }}
        />
        <button className="btn-commit" onClick={handleCommit} disabled={!commitMsg.trim()}>
          Commit
        </button>
      </div>

      <div className="git-sections">
        <div className="git-list-scroll">
          {unstaged.length === 0 && staged.length === 0 ? (
            <div className="git-empty-state">
              <div className="git-empty-icon">
                <i className="fa-solid fa-check"></i>
              </div>
              <strong>Working tree is clean</strong>
              <p>Everything in this workspace is committed or unchanged. Pull again or keep coding.</p>
            </div>
          ) : (
            <>
              <div className="git-section">
                <div className="git-section-header">
                  CHANGES ({unstaged.length})
                  {unstaged.length > 0 && (
                    <button className="add-all-btn" onClick={handleAddAll} title="Stage All">
                      <i className="fa-solid fa-plus"></i>
                    </button>
                  )}
                </div>
                {unstaged.length === 0 ? (
                  <div className="git-empty-msg">Nothing new in the working tree.</div>
                ) : (
                  <div className="git-section-body">
                    {unstaged.map(([path, status]) => (
                      <GitItem
                        key={path}
                        path={path}
                        status={status}
                        rootPath={rootPath}
                        staged={false}
                        selected={selectedChange?.filePath === path && !selectedChange.staged}
                        onSelect={() => setSelectedChange({ filePath: path, status, staged: false })}
                        onOpen={() => openFile(path)}
                        onStageAction={() => handleAdd(path)}
                        onDiscard={() => { void handleDiscard({ filePath: path, status, staged: false }) }}
                      />
                    ))}
                  </div>
                )}
              </div>

              {staged.length > 0 && (
                <div className="git-section">
                  <div className="git-section-header">STAGED CHANGES ({staged.length})</div>
                  <div className="git-section-body">
                    {staged.map(([path, status]) => (
                      <GitItem
                        key={path}
                        path={path}
                        status={status}
                        rootPath={rootPath}
                        staged
                        selected={selectedChange?.filePath === path && selectedChange.staged}
                        onSelect={() => setSelectedChange({ filePath: path, status, staged: true })}
                        onOpen={() => openFile(path)}
                        onStageAction={() => handleUnstage(path)}
                        onDiscard={() => { void handleDiscard({ filePath: path, status, staged: true }) }}
                      />
                    ))}
                  </div>
                </div>
              )}
            </>
          )}
        </div>

        <div className="git-preview">
          <div className="git-preview-head">
            {selectedChange && selectedMeta ? (
              <>
                <div className="git-preview-copy">
                  <span className="git-preview-eyebrow">Diff Preview</span>
                  <strong>{selectedName}</strong>
                  <span className="git-preview-path">{selectedRelativePath}</span>
                </div>
                <div className="git-preview-actions">
                  <span className="git-status-pill" style={{ color: selectedMeta.color }}>
                    {selectedMeta.label}
                  </span>
                  <button className="git-preview-btn" onClick={() => openFile(selectedChange.filePath)}>
                    <i className="fa-regular fa-file-lines"></i> Open
                  </button>
                  {!selectedChange.staged ? (
                    <button className="git-preview-btn primary" onClick={() => handleAdd(selectedChange.filePath)}>
                      <i className="fa-solid fa-plus"></i> Stage
                    </button>
                  ) : (
                    <button className="git-preview-btn" onClick={() => handleUnstage(selectedChange.filePath)}>
                      <i className="fa-solid fa-arrow-turn-up"></i> Unstage
                    </button>
                  )}
                  {selectedChange.status !== 'renamed' && (
                    <button className="git-preview-btn danger" onClick={() => { void handleDiscard(selectedChange) }}>
                      <i className="fa-regular fa-trash-can"></i> Discard
                    </button>
                  )}
                </div>
              </>
            ) : (
              <div className="git-preview-copy">
                <span className="git-preview-eyebrow">Diff Preview</span>
                <strong>Select a change to inspect it</strong>
                <span className="git-preview-path">A unified diff will appear here before you stage or commit.</span>
              </div>
            )}
          </div>

          <div className="git-diff-surface">
            {isDiffLoading ? (
              <div className="git-diff-empty">
                <i className="fa-solid fa-spinner fa-spin"></i>
                <span>Loading diff preview...</span>
              </div>
            ) : !selectedChange ? (
              <div className="git-diff-empty">
                <i className="fa-regular fa-file-code"></i>
                <span>No file selected yet.</span>
              </div>
            ) : (
              <>
                <div className="git-diff-code" role="log" aria-label="Git diff preview">
                  {visibleDiffLines.length === 0 ? (
                    <div className="git-diff-empty">
                      <i className="fa-regular fa-file-lines"></i>
                      <span>No diff available for this change.</span>
                    </div>
                  ) : (
                    visibleDiffLines.map((line, index) => (
                      <div key={`${selectedChange.filePath}-${index}`} className={classifyDiffLine(line)}>
                        {line || ' '}
                      </div>
                    ))
                  )}
                </div>
                {isDiffTruncated && (
                  <div className="git-diff-footnote">
                    Preview capped at the first {visibleDiffLines.length} lines to keep the panel fast.
                  </div>
                )}
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}

function GitItem({
  path,
  status,
  rootPath,
  staged,
  selected,
  onSelect,
  onOpen,
  onStageAction,
  onDiscard,
}: {
  path: string
  status: string
  rootPath: string
  staged: boolean
  selected: boolean
  onSelect: () => void
  onOpen: () => void
  onStageAction: () => void
  onDiscard: () => void
}) {
  const fileName = path.split(window.electronAPI.isWindows ? '\\' : '/').pop() || 'unknown'
  const relPath = getRelativePath(path, rootPath)
  const statusMeta = getStatusMeta(status, staged)

  return (
    <div className={`git-item ${selected ? 'selected' : ''}`} onClick={onSelect}>
      <div className="git-item-info">
        <span className="git-item-name" style={{ color: statusMeta.color }}>{fileName}</span>
        <span className="git-item-path">{relPath}</span>
      </div>
      <div className="git-item-actions">
        <button
          className="git-open-btn"
          onClick={(event) => {
            event.stopPropagation()
            onOpen()
          }}
          title="Open file"
        >
          <i className="fa-regular fa-file-lines"></i>
        </button>
        <span className="git-item-status" style={{ color: statusMeta.color }}>
          {statusMeta.short}
        </span>
        {!staged && (
          <button
            className="git-add-btn"
            onClick={(event) => {
              event.stopPropagation()
              onStageAction()
            }}
            title="Stage Change"
          >
            +
          </button>
        )}
        {staged && (
          <button
            className="git-add-btn"
            onClick={(event) => {
              event.stopPropagation()
              onStageAction()
            }}
            title="Unstage Change"
          >
            -
          </button>
        )}
        {status !== 'renamed' && (
          <button
            className="git-discard-btn"
            onClick={(event) => {
              event.stopPropagation()
              onDiscard()
            }}
            title="Discard Change"
          >
            <i className="fa-regular fa-trash-can"></i>
          </button>
        )}
      </div>
    </div>
  )
}
