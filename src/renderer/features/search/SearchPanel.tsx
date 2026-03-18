import React, { useState } from 'react'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { useEditorStore } from '../../store/editorStore'

interface SearchResult {
  path: string
  name: string
  line: number
  match: string
}

export function SearchPanel() {
  const [query, setQuery] = useState('')
  const [replaceQuery, setReplaceQuery] = useState('')
  const [showReplace, setShowReplace] = useState(false)
  const [results, setResults] = useState<SearchResult[]>([])
  const [searching, setSearching] = useState(false)
  const { rootPath, refreshTree } = useWorkspaceStore()
  const openFile = useEditorStore(s => s.openFile)

  const handleSearch = async () => {
    if (!query.trim() || !rootPath) return
    setSearching(true)
    try {
      const res = await window.electronAPI.searchFiles(query.trim(), rootPath)
      setResults(res)
    } catch (err) {
      console.error(err)
    } finally {
      setSearching(false)
    }
  }

  const handleReplaceAll = async () => {
    if (!query.trim() || !rootPath) return
    if (!confirm(`Replace all occurrences of "${query}" with "${replaceQuery}" across the project?`)) return
    
    setSearching(true)
    try {
      const result = await window.electronAPI.replaceInFiles(query, replaceQuery, rootPath)
      if (result.success) {
        alert(`Replaced ${result.count} files.`)
        handleSearch()
        await refreshTree()
      } else {
        alert(`Replace failed: ${result.error}`)
      }
    } catch (err) {
      console.error(err)
    } finally {
      setSearching(false)
    }
  }

  return (
    <div className="search-panel">
      <div className="sidebar-header">
        <span className="sidebar-title">SEARCH</span>
      </div>
      
      <div className="search-input-wrapper" style={{ flexDirection: 'column', gap: '8px' }}>
        <div style={{ display: 'flex', gap: '8px', width: '100%' }}>
          <button 
            className="sidebar-toggle-btn" 
            onClick={() => setShowReplace(!showReplace)}
            title="Toggle Replace"
            style={{ padding: '0 8px', background: showReplace ? 'var(--acc)' : 'transparent' }}
          >
            <i className={`fa-solid fa-chevron-${showReplace ? 'down' : 'right'}`}></i>
          </button>
          <input 
            className="search-input"
            placeholder="Search in files..."
            value={query}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && handleSearch()}
          />
        </div>
        
        {showReplace && (
          <div style={{ display: 'flex', gap: '8px', width: '100%', paddingLeft: '28px' }}>
            <input 
              className="search-input"
              placeholder="Replace with..."
              value={replaceQuery}
              onChange={e => setReplaceQuery(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && handleReplaceAll()}
            />
          </div>
        )}

        <div style={{ display: 'flex', gap: '8px', width: '100%', paddingLeft: '28px' }}>
          <button className="search-btn" onClick={handleSearch} disabled={searching} style={{ flex: 1 }}>
            {searching ? '...' : 'Find All'}
          </button>
          {showReplace && (
            <button className="search-btn" onClick={handleReplaceAll} disabled={searching || !query} style={{ flex: 1, background: 'var(--warning)' }}>
              Replace All
            </button>
          )}
        </div>
      </div>

      <div className="search-results">
        {results.length === 0 ? (
          <div className="search-empty">{searching ? 'Searching...' : 'No results'}</div>
        ) : (
          results.map((r, i) => (
            <div key={i} className="search-item" onClick={() => openFile(r.path)} title={r.path}>
              <div className="search-file">📄 {r.name} <span className="search-line">line {r.line}</span></div>
              <div className="search-match">{r.match}</div>
            </div>
          ))
        )}
      </div>
    </div>
  )
}
