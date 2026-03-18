import React, { useState, useEffect, useCallback } from 'react'
import { useWorkspaceStore } from '../../store/workspaceStore'

export function GitPanel() {
  const { rootPath, gitStatus, refreshTree } = useWorkspaceStore()
  const [commitMsg, setCommitMsg] = useState('')
  const [isRefreshing, setIsRefreshing] = useState(false)

  const handleRefresh = useCallback(async () => {
    setIsRefreshing(true)
    await refreshTree()
    setIsRefreshing(false)
  }, [refreshTree])

  const handleAdd = async (path: string) => {
    const success = await window.electronAPI.gitAdd(rootPath!, [path])
    if (success) await refreshTree()
  }

  const handleAddAll = async () => {
    const success = await window.electronAPI.gitAdd(rootPath!, ['.'])
    if (success) await refreshTree()
  }

  const handleCommit = async () => {
    if (!commitMsg.trim()) return
    const success = await window.electronAPI.gitCommit(rootPath!, commitMsg)
    if (success) {
      setCommitMsg('')
      await refreshTree()
    } else {
      alert('Commit failed. Check console.')
    }
  }

  const handlePush = async () => {
    const success = await window.electronAPI.gitPush(rootPath!)
    if (success) alert('Push successful!')
    else alert('Push failed!')
  }

  const handlePull = async () => {
    const success = await window.electronAPI.gitPull(rootPath!)
    if (success) {
       alert('Pull successful!')
       await refreshTree()
    } else alert('Pull failed!')
  }

  const staged = Object.entries(gitStatus).filter(([_, status]) => status === 'staged')
  const unstaged = Object.entries(gitStatus).filter(([_, status]) => status !== 'staged')

  if (!rootPath) {
    return (
      <div className="git-panel-empty">
        <p>No folder open</p>
      </div>
    )
  }

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
        {staged.length > 0 && (
          <div className="git-section">
            <div className="git-section-header">STAGED CHANGES ({staged.length})</div>
            {staged.map(([path, status]) => (
              <GitItem key={path} path={path} status={status} rootPath={rootPath} onAdd={() => {}} />
            ))}
          </div>
        )}

        <div className="git-section">
          <div className="git-section-header">
            CHANGES ({unstaged.length})
            {unstaged.length > 0 && <button className="add-all-btn" onClick={handleAddAll} title="Stage All">+</button>}
          </div>
          {unstaged.length === 0 ? (
            <div className="git-empty-msg">No changes detected</div>
          ) : (
            unstaged.map(([path, status]) => (
              <GitItem key={path} path={path} status={status} rootPath={rootPath!} onAdd={() => handleAdd(path)} />
            ))
          )}
        </div>
      </div>
    </div>
  )
}

function GitItem({ path, status, rootPath, onAdd }: { path: string, status: string, rootPath: string, onAdd: () => void }) {
  const fileName = path.split(window.electronAPI.isWindows ? '\\' : '/').pop() || 'unknown'
  const relPath = path.replace(rootPath, '').substring(1)

  const getStatusColor = () => {
    switch (status) {
      case 'modified': return '#e2c08d'
      case 'added':
      case 'untracked': return '#8be9fd'
      case 'staged': return '#50fa7b'
      case 'deleted': return '#ff5555'
      default: return 'inherit'
    }
  }

  return (
    <div className="git-item">
      <div className="git-item-info">
        <span className="git-item-name" style={{ color: getStatusColor() }}>{fileName}</span>
        <span className="git-item-path">{relPath}</span>
      </div>
      <div className="git-item-actions">
        <span className="git-item-status" style={{ color: getStatusColor() }}>
          {status === 'modified' ? 'M' : status === 'added' || status === 'untracked' ? 'U' : status === 'staged' ? 'A' : 'D'}
        </span>
        {status !== 'staged' && status !== 'deleted' && (
          <button className="git-add-btn" onClick={onAdd} title="Stage Change">+</button>
        )}
      </div>
    </div>
  )
}
