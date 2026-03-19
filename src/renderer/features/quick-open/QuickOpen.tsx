import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useUiStore } from '../../store/uiStore'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { useEditorStore } from '../../store/editorStore'
import { buildQuickOpenItems, parseQuickOpenQuery } from './utils'

function getFileIconClass(fileName: string): string {
  const ext = fileName.split('.').pop()?.toLowerCase() ?? ''
  const icons: Record<string, string> = {
    ts: 'fa-solid fa-circle-chevron-right',
    tsx: 'fa-brands fa-react',
    js: 'fa-brands fa-js',
    jsx: 'fa-brands fa-react',
    json: 'fa-solid fa-code',
    html: 'fa-brands fa-html5',
    css: 'fa-brands fa-css3-alt',
    py: 'fa-brands fa-python',
    md: 'fa-solid fa-file-lines',
    sh: 'fa-solid fa-terminal',
  }

  return icons[ext] ?? 'fa-regular fa-file'
}

function renderHighlightedText(text: string, matches: number[]) {
  if (matches.length === 0) return text

  const highlighted = new Set(matches)
  return text.split('').map((character, index) => (
    <span
      key={`${character}-${index}`}
      className={highlighted.has(index) ? 'quick-open-highlight' : undefined}
    >
      {character}
    </span>
  ))
}

type QuickOpenEntry =
  | { id: string; type: 'line-jump'; filePath: string; fileName: string; lineNumber: number }
  | {
      id: string
      type: 'file'
      filePath: string
      fileName: string
      relativePath: string
      isRecent: boolean
      lineNumber?: number
      fileNameMatches: number[]
      pathMatches: number[]
    }

export function QuickOpen() {
  const show = useUiStore((state) => state.showQuickOpen)
  const toggle = useUiStore((state) => state.toggleQuickOpen)
  const { rootPath, openFolder, setRootPath, recentWorkspaces, revision } = useWorkspaceStore()
  const openFile = useEditorStore((state) => state.openFile)
  const recentFiles = useEditorStore((state) => state.recentFiles)
  const getActiveTab = useEditorStore((state) => state.getActiveTab)

  const [query, setQuery] = useState('')
  const [files, setFiles] = useState<string[]>([])
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  const workspaceName = rootPath?.replace(/\\/g, '/').split('/').pop() || 'Workspace'
  const activeTab = getActiveTab()
  const parsedQuery = useMemo(() => parseQuickOpenQuery(query), [query])
  const { items } = useMemo(
    () => buildQuickOpenItems(files, rootPath, query, recentFiles, 12),
    [files, query, recentFiles, rootPath]
  )

  const entries = useMemo<QuickOpenEntry[]>(() => {
    const nextEntries: QuickOpenEntry[] = []

    if (parsedQuery.lineNumber && !parsedQuery.fileQuery && activeTab) {
      nextEntries.push({
        id: `line-${activeTab.path}-${parsedQuery.lineNumber}`,
        type: 'line-jump',
        filePath: activeTab.path,
        fileName: activeTab.name,
        lineNumber: parsedQuery.lineNumber,
      })
    }

    nextEntries.push(
      ...items.map((item) => ({
        id: item.path,
        type: 'file' as const,
        filePath: item.path,
        fileName: item.fileName,
        relativePath: item.relativePath,
        isRecent: item.isRecent,
        lineNumber: parsedQuery.lineNumber,
        fileNameMatches: item.fileNameMatches,
        pathMatches: item.pathMatches,
      }))
    )

    return nextEntries
  }, [activeTab, items, parsedQuery.fileQuery, parsedQuery.lineNumber])

  const loadFiles = useCallback(async () => {
    if (!rootPath) {
      setFiles([])
      return
    }

    const allFiles = await window.electronAPI.getAllFiles(rootPath)
    setFiles(allFiles)
  }, [rootPath])

  useEffect(() => {
    if (show) {
      setQuery('')
      setSelectedIndex(0)
      inputRef.current?.focus()
    } else {
      setFiles([])
    }
  }, [show])

  useEffect(() => {
    if (!show) return
    void loadFiles()
  }, [loadFiles, revision, show])

  useEffect(() => {
    setSelectedIndex((current) => {
      if (entries.length === 0) return 0
      return Math.min(current, entries.length - 1)
    })
  }, [entries])

  const openAtLocation = async (filePath: string, lineNumber?: number) => {
    if (lineNumber) {
      useUiStore.getState().setPendingEditorTarget({ filePath, line: lineNumber })
    }

    await openFile(filePath)
    toggle()
  }

  const executeEntry = async (entry: QuickOpenEntry | undefined) => {
    if (!entry) return

    if (entry.type === 'line-jump') {
      await openAtLocation(entry.filePath, entry.lineNumber)
      return
    }

    await openAtLocation(entry.filePath, entry.lineNumber)
  }

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape') {
      toggle()
      return
    }

    if (event.key === 'ArrowDown') {
      event.preventDefault()
      if (entries.length === 0) return
      setSelectedIndex((current) => (current + 1) % entries.length)
      return
    }

    if (event.key === 'ArrowUp') {
      event.preventDefault()
      if (entries.length === 0) return
      setSelectedIndex((current) => (current - 1 + entries.length) % entries.length)
      return
    }

    if (event.key === 'Enter') {
      event.preventDefault()
      void executeEntry(entries[selectedIndex])
    }
  }

  if (!show) return null

  return (
    <div className="quick-open-overlay" onClick={toggle}>
      <div className="quick-open-modal" onClick={(event) => event.stopPropagation()}>
        <div className="quick-open-head">
          <div className="quick-open-copy">
            <span className="quick-open-eyebrow">Quick Open</span>
            <strong>
              {rootPath ? `Jump to any file in ${workspaceName}` : 'Open a workspace to search files'}
            </strong>
            <span className="quick-open-meta">
              {rootPath
                ? `${files.length} indexed files • Recent files are boosted • Use :120 to jump to a line`
                : 'Choose a project folder first, or jump back into a recent workspace below.'}
            </span>
          </div>
          {rootPath && (
            <div className="quick-open-badge">
              <span>Visible</span>
              <strong>{entries.length}</strong>
            </div>
          )}
        </div>

        <div className="quick-open-search">
          <span className="quick-open-search-icon">
            <i className="fa-solid fa-magnifying-glass"></i>
          </span>
          <input
            ref={inputRef}
            className="quick-open-input"
            placeholder="Type a filename, path, or foo.ts:120..."
            value={query}
            onChange={(event) => {
              setQuery(event.target.value)
              setSelectedIndex(0)
            }}
            onKeyDown={handleKeyDown}
          />
          {query && (
            <button className="quick-open-clear" onClick={() => setQuery('')} title="Clear search">
              <i className="fa-solid fa-xmark"></i>
            </button>
          )}
        </div>

        {!rootPath ? (
          <div className="quick-open-empty-state">
            <div className="quick-open-empty-icon">
              <i className="fa-solid fa-folder-open"></i>
            </div>
            <strong>No workspace open yet</strong>
            <span>Open a folder to index files instantly, or jump back into one of your recent workspaces.</span>
            <button
              className="open-folder-btn quick-open-action"
              onClick={async () => {
                await openFolder()
              }}
            >
              <i className="fa-solid fa-folder-open"></i> Open Folder
            </button>
            {recentWorkspaces.length > 0 && (
              <div className="quick-open-recents">
                <span className="quick-open-recents-title">Recent Workspaces</span>
                {recentWorkspaces.map((workspacePath) => {
                  const label = workspacePath.replace(/\\/g, '/').split('/').pop() || workspacePath
                  return (
                    <button
                      key={workspacePath}
                      type="button"
                      className="quick-open-recent-workspace"
                      onClick={async () => {
                        await setRootPath(workspacePath)
                      }}
                    >
                      <span>{label}</span>
                      <small>{workspacePath}</small>
                    </button>
                  )
                })}
              </div>
            )}
          </div>
        ) : (
          <>
            <div className="quick-open-results">
              {entries.map((entry, index) => {
                if (entry.type === 'line-jump') {
                  return (
                    <button
                      key={entry.id}
                      type="button"
                      className={`quick-open-item quick-open-item-line ${index === selectedIndex ? 'active' : ''}`}
                      onMouseEnter={() => setSelectedIndex(index)}
                      onClick={() => { void executeEntry(entry) }}
                    >
                      <span className="quick-open-item-icon">
                        <i className="fa-solid fa-arrow-turn-down"></i>
                      </span>
                      <span className="quick-open-item-copy">
                        <span className="quick-open-item-row">
                          <span className="quick-open-item-name">Go to line {entry.lineNumber}</span>
                          <span className="quick-open-item-ext">LINE</span>
                        </span>
                        <span className="quick-open-item-path">{entry.fileName}</span>
                      </span>
                      <span className="quick-open-item-index">NOW</span>
                    </button>
                  )
                }

                return (
                  <button
                    key={entry.id}
                    type="button"
                    className={`quick-open-item ${index === selectedIndex ? 'active' : ''}`}
                    onMouseEnter={() => setSelectedIndex(index)}
                    onClick={() => { void executeEntry(entry) }}
                  >
                    <span className="quick-open-item-icon">
                      <i className={getFileIconClass(entry.fileName)}></i>
                    </span>
                    <span className="quick-open-item-copy">
                      <span className="quick-open-item-row">
                        <span className="quick-open-item-name">
                          {renderHighlightedText(entry.fileName, entry.fileNameMatches)}
                        </span>
                        <span className="quick-open-item-ext">
                          {entry.isRecent ? 'RECENT' : (entry.fileName.includes('.') ? entry.fileName.split('.').pop()?.toUpperCase() : 'FILE')}
                        </span>
                        {entry.lineNumber && <span className="quick-open-item-jump">:{entry.lineNumber}</span>}
                      </span>
                      <span className="quick-open-item-path">
                        {renderHighlightedText(entry.relativePath, entry.pathMatches)}
                      </span>
                    </span>
                    <span className="quick-open-item-index">{String(index + 1).padStart(2, '0')}</span>
                  </button>
                )
              })}

              {entries.length === 0 && (
                <div className="quick-open-empty-state compact">
                  <div className="quick-open-empty-icon">
                    <i className="fa-solid fa-file-circle-question"></i>
                  </div>
                  <strong>No files match “{query}”</strong>
                  <span>Try a shorter fuzzy search, a folder segment, or append :120 to jump after opening a file.</span>
                </div>
              )}
            </div>

            <div className="quick-open-footer">
              <span><kbd>Enter</kbd> Open file</span>
              <span><kbd>↑</kbd> <kbd>↓</kbd> Move selection</span>
              <span><kbd>:120</kbd> Jump to line</span>
              <span><kbd>Esc</kbd> Close</span>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
