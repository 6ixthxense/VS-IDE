import React, { Suspense, lazy, useState, useCallback, useEffect, useRef } from 'react'
import { createPortal } from 'react-dom'
import type { FileEntry } from '@shared/types/file'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useEditorStore } from '../store/editorStore'
import { useUiStore } from '../store/uiStore'
import { confirmAction, showErrorToast, showInfoToast, showSuccessToast } from '../store/feedbackStore'

const SearchPanel = lazy(() =>
  import('../features/search/SearchPanel').then((module) => ({ default: module.SearchPanel }))
)
const GitPanel = lazy(() =>
  import('../features/git/GitPanel').then((module) => ({ default: module.GitPanel }))
)

interface FileTreeProps {
  entries: FileEntry[]
  depth?: number
  onNewFileRequest?: (entry: FileEntry) => void
  onNewFolderRequest?: (entry: FileEntry) => void
}

interface ContextMenuProps {
  x: number
  y: number
  entry: FileEntry
  onClose: () => void
  onAction: (action: string, entry: FileEntry) => void
}

function ContextMenu({ x, y, entry, onClose, onAction }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose()
      }
    }
    window.addEventListener('mousedown', handleClick)
    return () => window.removeEventListener('mousedown', handleClick)
  }, [onClose])

  return createPortal(
    <div 
      ref={menuRef}
      className="context-menu" 
      style={{ left: x, top: y }}
      onClick={(e) => e.stopPropagation()}
    >
      {entry.isDirectory && (
        <>
          <div className="context-menu-item" onClick={() => onAction('newFile', entry)}>
            <i className="fa-solid fa-file-circle-plus"></i> New File
          </div>
          <div className="context-menu-item" onClick={() => onAction('newFolder', entry)}>
            <i className="fa-solid fa-folder-plus"></i> New Folder
          </div>
          <div className="context-menu-separator"></div>
        </>
      )}
      <div className="context-menu-item" onClick={() => onAction('rename', entry)}>
        <i className="fa-solid fa-pen-to-square"></i> Rename
      </div>
      <div className="context-menu-item" onClick={() => onAction('copyPath', entry)}>
        <i className="fa-solid fa-copy"></i> Copy Path
      </div>
      <div className="context-menu-item" onClick={() => onAction('reveal', entry)}>
        <i className="fa-solid fa-folder-tree"></i> Reveal in Explorer
      </div>
      <div className="context-menu-separator"></div>
      <div className="context-menu-item danger" onClick={() => onAction('delete', entry)}>
        <i className="fa-solid fa-trash"></i> Delete
      </div>
    </div>,
    document.body
  )
}

function getFileIconClass(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
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
    folder: 'fa-solid fa-folder',
    folderOpen: 'fa-solid fa-folder-open'
  }
  return icons[ext] ?? 'fa-regular fa-file'
}

function getFileIconColor(name: string): string {
  const ext = name.split('.').pop()?.toLowerCase() ?? ''
  const colors: Record<string, string> = {
    ts: '#007acc', tsx: '#00d8ff', js: '#f7df1e', jsx: '#00d8ff',
    json: '#ffb300', html: '#e44d26', css: '#244ff2', py: '#3776ab',
    md: '#03a9f4', sh: '#4caf50'
  }
  return colors[ext] ?? '#a0a0a0'
}

function FileTree({ entries, depth = 0, onNewFileRequest, onNewFolderRequest }: FileTreeProps) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  const [childMap, setChildMap] = useState<Record<string, FileEntry[]>>({})
  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, entry: FileEntry } | null>(null)
  const [renamingPath, setRenamingPath] = useState<string | null>(null)
  const [newName, setNewName] = useState('')
  
  const openFile = useEditorStore(s => s.openFile)
  const refreshTree = useWorkspaceStore(s => s.refreshTree)
  const gitStatus = useWorkspaceStore(s => s.gitStatus)

  const getGitColor = (path: string) => {
    const status = gitStatus[path]
    switch (status) {
      case 'modified': return '#e2c08d' // Yellow
      case 'added': return '#50fa7b'    // Green
      case 'untracked': return '#8be9fd' // Cyan
      case 'staged': return '#50fa7b'
      case 'deleted': return '#ff5555'   // Red
      default: return 'inherit'
    }
  }

  const getGitStatusSymbol = (path: string) => {
    const status = gitStatus[path]
    switch (status) {
      case 'modified': return 'M'
      case 'added':
      case 'untracked': return 'U'
      case 'staged': return 'A'
      case 'deleted': return 'D'
      default: return null
    }
  }

  const toggle = useCallback(async (entry: FileEntry) => {
    const key = entry.path
    if (!expanded[key]) {
      if (!childMap[key]) {
        const children = await window.electronAPI.readDir(entry.path)
        setChildMap(m => ({ ...m, [key]: children }))
      }
      setExpanded(e => ({ ...e, [key]: true }))
    } else {
      setExpanded(e => ({ ...e, [key]: false }))
    }
  }, [expanded, childMap])

  const handleDelete = async (path: string, name: string) => {
    const confirmed = await confirmAction({
      title: 'Delete item?',
      message: `This will permanently delete "${name}" from the workspace.`,
      confirmLabel: 'Delete',
      cancelLabel: 'Keep',
      tone: 'warning',
    })
    if (!confirmed) return

    const result = await (window.electronAPI as any).deletePath(path)
    if (result.success) {
      await refreshTree()
      showSuccessToast(`"${name}" was removed.`, 'Deleted')
    } else {
      showErrorToast(`Delete failed: ${result.error}`, 'Delete failed')
    }
  }

  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault()
    e.stopPropagation()
    setContextMenu({ x: e.clientX, y: e.clientY, entry })
  }

  const handleRename = async (entry: FileEntry, name: string) => {
    if (!name.trim() || name === entry.name) {
      setRenamingPath(null)
      return
    }
    const parent = entry.path.substring(0, entry.path.lastIndexOf(window.electronAPI.isWindows ? '\\' : '/'))
    const newPath = `${parent}${window.electronAPI.isWindows ? '\\' : '/'}${name.trim()}`
    
    const result = await window.electronAPI.renamePath(entry.path, newPath)
    if (result.success) {
      setRenamingPath(null)
      await refreshTree()
      showSuccessToast(`Renamed to "${name.trim()}".`, 'Rename complete')
    } else {
      showErrorToast(`Rename failed: ${result.error}`, 'Rename failed')
    }
  }

  const handleAction = async (action: string, entry: FileEntry) => {
    setContextMenu(null)
    switch (action) {
      case 'delete':
        handleDelete(entry.path, entry.name)
        break
      case 'rename':
        setRenamingPath(entry.path)
        setNewName(entry.name)
        break
      case 'copyPath':
        navigator.clipboard.writeText(entry.path)
        showInfoToast('Path copied to clipboard.', 'Copied')
        break
      case 'reveal':
        window.electronAPI.revealInExplorer(entry.path)
        break
      case 'newFile':
        onNewFileRequest?.(entry) 
        break
      case 'newFolder':
        onNewFolderRequest?.(entry)
        break
    }
  }

  const handleDragStart = (e: React.DragEvent, entry: FileEntry) => {
    e.dataTransfer.setData('text/plain', entry.path)
    e.dataTransfer.effectAllowed = 'move'
  }

  const handleDragOver = (e: React.DragEvent, entry: FileEntry) => {
    if (entry.isDirectory) {
      e.preventDefault()
      e.dataTransfer.dropEffect = 'move'
    }
  }

  const handleDrop = async (e: React.DragEvent, targetEntry: FileEntry) => {
    e.preventDefault()
    const sourcePath = e.dataTransfer.getData('text/plain')
    if (!sourcePath || sourcePath === targetEntry.path) return

    const result = await window.electronAPI.movePath(sourcePath, targetEntry.path)
    if (result.success) {
      await refreshTree()
      showSuccessToast('Item moved successfully.', 'Move complete')
    } else {
      showErrorToast(`Move failed: ${result.error}`, 'Move failed')
    }
  }

  return (
    <>
      {entries.map(entry => (
        <div key={entry.path}>
          <div
            className="file-item"
            style={{ paddingLeft: `${12 + depth * 14}px` }}
            onClick={() => entry.isDirectory ? toggle(entry) : openFile(entry.path)}
            onContextMenu={(e) => handleContextMenu(e, entry)}
            draggable
            onDragStart={(e) => handleDragStart(e, entry)}
            onDragOver={(e) => handleDragOver(e, entry)}
            onDrop={(e) => handleDrop(e, entry)}
            title={entry.path}
          >
            <span className="file-icon">
              {entry.isDirectory ? (
                <i 
                  className={expanded[entry.path] ? 'fa-solid fa-folder-open' : 'fa-solid fa-folder'} 
                  style={{ color: '#d0b05c', fontSize: '13px' }}
                />
              ) : (
                <i 
                  className={getFileIconClass(entry.name)} 
                  style={{ color: getFileIconColor(entry.name), fontSize: '13px' }}
                />
              )}
            </span>
            {renamingPath === entry.path ? (
              <input 
                className="file-rename-input"
                autoFocus
                value={newName}
                onChange={e => setNewName(e.target.value)}
                onBlur={() => handleRename(entry, newName)}
                onKeyDown={e => {
                  if (e.key === 'Enter') handleRename(entry, newName)
                  if (e.key === 'Escape') setRenamingPath(null)
                }}
                onClick={e => e.stopPropagation()}
              />
            ) : (
              <span className="file-name" style={{ color: getGitColor(entry.path) }}>
                {entry.name}
              </span>
            )}
            <div className="file-item-actions">
              {getGitStatusSymbol(entry.path) && (
                <span className="git-status-symbol" title={gitStatus[entry.path]} style={{ color: getGitColor(entry.path) }}>
                  {getGitStatusSymbol(entry.path)}
                </span>
              )}
              <button 
                onClick={(e) => { e.stopPropagation(); handleDelete(entry.path, entry.name); }} 
                title="Delete"
              >
                <i className="fa-solid fa-trash"></i>
              </button>
            </div>
          </div>
          {entry.isDirectory && expanded[entry.path] && childMap[entry.path] && (
            <div className="file-tree-sub" style={{ overflow: 'hidden' }}>
              <FileTree 
                entries={childMap[entry.path]} 
                depth={depth + 1} 
                onNewFileRequest={onNewFileRequest}
                onNewFolderRequest={onNewFolderRequest}
              />
            </div>
          )}
        </div>
      ))}

      {contextMenu && (
        <ContextMenu 
          {...contextMenu} 
          onClose={() => setContextMenu(null)}
          onAction={handleAction}
        />
      )}
    </>
  )
}

export function Sidebar() {
  const { rootPath, tree, isLoading, openFolder, refreshTree } = useWorkspaceStore()
  const { activeSidebarView } = useUiStore()
  const [newFileModal, setNewFileModal] = useState(false)
  const [newFolderModal, setNewFolderModal] = useState(false)
  const [newFileName, setNewFileName] = useState('')
  const [newFolderName, setNewFolderName] = useState('')
  const [targetDir, setTargetDir] = useState<string | null>(null)

  const handleNewFile = async () => {
    const parent = targetDir || rootPath
    if (!parent || !newFileName.trim()) return
    const result = await window.electronAPI.createFile(parent, newFileName.trim())
    if (result.success && result.path) {
      setNewFileModal(false)
      setNewFileName('')
      setTargetDir(null)
      await refreshTree()
      await useEditorStore.getState().openFile(result.path)
      showSuccessToast(`Created "${result.path.split(window.electronAPI.isWindows ? '\\' : '/').pop() || 'file'}".`, 'File created')
    } else if (!result.success) {
      showErrorToast(`Create file failed: ${result.error}`, 'Create file failed')
    }
  }

  const handleNewFolder = async () => {
    const parent = targetDir || rootPath
    if (!parent || !newFolderName.trim()) return
    const result = await window.electronAPI.createDirectory(parent, newFolderName.trim())
    if (result.success) {
      setNewFolderModal(false)
      setNewFolderName('')
      setTargetDir(null)
      await refreshTree()
      showSuccessToast(`Created "${newFolderName.trim()}".`, 'Folder created')
    } else {
      showErrorToast(`Create folder failed: ${result.error}`, 'Create folder failed')
    }
  }

  const onNewFileRequest = (entry: FileEntry) => {
    setTargetDir(entry.path)
    setNewFileModal(true)
  }

  const onNewFolderRequest = (entry: FileEntry) => {
    setTargetDir(entry.path)
    setNewFolderModal(true)
  }

  const workspaceName = rootPath?.replace(/\\/g, '/').split('/').pop() || 'workspace'

  return (
    <aside className="sidebar">
      {activeSidebarView === 'explorer' && (
        <>
          <div className="sidebar-header">
            <span className="sidebar-title">EXPLORER</span>
            <div className="sidebar-actions">
              <button onClick={openFolder} title="Open Folder">
                <i className="fa-solid fa-folder-open"></i>
              </button>
              {rootPath && (
                <>
                  <button onClick={() => { setTargetDir(null); setNewFileModal(true); }} title="New File">
                    <i className="fa-solid fa-file-circle-plus"></i>
                  </button>
                  <button onClick={() => { setTargetDir(null); setNewFolderModal(true); }} title="New Folder">
                    <i className="fa-solid fa-folder-plus"></i>
                  </button>
                  <button onClick={refreshTree} title="Refresh">
                    <i className="fa-solid fa-arrow-rotate-right"></i>
                  </button>
                </>
              )}
            </div>
          </div>

          {!rootPath ? (
            <div className="sidebar-empty">
              <div className="panel-empty-card">
                <span className="panel-empty-eyebrow">Explorer</span>
                <strong>Open a folder to start building</strong>
                <p>Browse files, create new folders, drag items around, and keep git status visible in one place.</p>
                <button className="open-folder-btn" onClick={openFolder}>
                  <i className="fa-solid fa-folder-open"></i> Open Folder
                </button>
                <button
                  className="welcome-secondary-btn sidebar-empty-btn"
                  onClick={() => useUiStore.setState({ showCommandPalette: true })}
                >
                  <i className="fa-solid fa-wand-magic-sparkles"></i> Command Palette
                </button>
              </div>
            </div>
          ) : (
            <div className="file-tree">
              {isLoading ? (
                <div className="loading">Loading...</div>
              ) : tree.length === 0 ? (
                <div className="sidebar-empty sidebar-empty-workspace">
                  <div className="panel-empty-card">
                    <span className="panel-empty-eyebrow">{workspaceName}</span>
                    <strong>This folder is ready for its first file</strong>
                    <p>Create a starter file or add a folder structure so Quick Open and Search have something to work with.</p>
                    <button
                      className="open-folder-btn"
                      onClick={() => {
                        setTargetDir(null)
                        setNewFileModal(true)
                      }}
                    >
                      <i className="fa-solid fa-file-circle-plus"></i> New File
                    </button>
                    <button
                      className="welcome-secondary-btn sidebar-empty-btn"
                      onClick={() => {
                        setTargetDir(null)
                        setNewFolderModal(true)
                      }}
                    >
                      <i className="fa-solid fa-folder-plus"></i> New Folder
                    </button>
                  </div>
                </div>
              ) : (
                <FileTree 
                  entries={tree} 
                  onNewFileRequest={onNewFileRequest}
                  onNewFolderRequest={onNewFolderRequest}
                />
              )}
            </div>
          )}

          {newFileModal && createPortal(
            <div className="modal-overlay" onClick={() => setNewFileModal(false)}>
              <div className="modal" onClick={e => e.stopPropagation()}>
                <h3>New File</h3>
                <input
                  autoFocus
                  value={newFileName}
                  onChange={e => setNewFileName(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter') handleNewFile(); if (e.key === 'Escape') setNewFileModal(false) }}
                  placeholder="e.g. app.ts, style.css"
                />
                <div className="modal-actions">
                  <button className="btn-cancel" onClick={() => setNewFileModal(false)}>Cancel</button>
                  <button className="btn-confirm" onClick={handleNewFile}>Create</button>
                </div>
              </div>
            </div>,
            document.body
          )}

          {newFolderModal && createPortal(
            <div className="modal-overlay" onClick={() => setNewFolderModal(false)}>
              <div className="modal" onClick={e => e.stopPropagation()}>
                <h3>New Folder</h3>
                <input
                  autoFocus
                  value={newFolderName}
                  onChange={e => setNewFolderName(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter') handleNewFolder(); if (e.key === 'Escape') setNewFolderModal(false) }}
                  placeholder="e.g. components, styles"
                />
                <div className="modal-actions">
                  <button className="btn-cancel" onClick={() => setNewFolderModal(false)}>Cancel</button>
                  <button className="btn-confirm" onClick={handleNewFolder}>Create</button>
                </div>
              </div>
            </div>,
            document.body
          )}
        </>
      )}
      {activeSidebarView === 'search' && (
        <Suspense fallback={<div className="loading">Loading search...</div>}>
          <SearchPanel />
        </Suspense>
      )}
      {activeSidebarView === 'git' && (
        <Suspense fallback={<div className="loading">Loading source control...</div>}>
          <GitPanel />
        </Suspense>
      )}
    </aside>
  )
}
