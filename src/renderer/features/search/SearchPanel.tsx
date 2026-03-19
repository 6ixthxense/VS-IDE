import React, { useCallback, useEffect, useMemo, useState } from 'react'
import type { SearchOptions, SearchResult } from '@shared/types/ipc'
import { useWorkspaceStore } from '../../store/workspaceStore'
import { useEditorStore } from '../../store/editorStore'
import { confirmAction, showErrorToast, showInfoToast, showSuccessToast } from '../../store/feedbackStore'
import { useUiStore } from '../../store/uiStore'
import {
  buildReplacePreview,
  groupSearchResults,
  persistSearchHistory,
  readSearchHistory,
  touchSearchHistory,
} from './utils'

export function SearchPanel() {
  const [query, setQuery] = useState('')
  const [replaceQuery, setReplaceQuery] = useState('')
  const [showReplace, setShowReplace] = useState(false)
  const [results, setResults] = useState<SearchResult[]>([])
  const [searching, setSearching] = useState(false)
  const [caseSensitive, setCaseSensitive] = useState(false)
  const [wholeWord, setWholeWord] = useState(false)
  const [useRegex, setUseRegex] = useState(false)
  const [includePattern, setIncludePattern] = useState('')
  const [excludePattern, setExcludePattern] = useState('')
  const [searchHistory, setSearchHistory] = useState(() => readSearchHistory())
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({})
  const [lastExecutedQuery, setLastExecutedQuery] = useState('')
  const [lastSearchedRevision, setLastSearchedRevision] = useState<number | null>(null)
  const { rootPath, refreshTree, revision } = useWorkspaceStore()
  const openFile = useEditorStore(s => s.openFile)
  const setPendingEditorTarget = useUiStore((s) => s.setPendingEditorTarget)
  const activeSidebarView = useUiStore((s) => s.activeSidebarView)
  const searchOptions = useMemo<SearchOptions>(() => ({
    caseSensitive,
    wholeWord,
    useRegex,
    includePattern,
    excludePattern,
  }), [caseSensitive, excludePattern, includePattern, useRegex, wholeWord])
  const groupedResults = useMemo(() => groupSearchResults(results, rootPath), [results, rootPath])
  const resultLabel = searching
    ? 'Scanning workspace...'
    : lastExecutedQuery
      ? `${results.length} result${results.length === 1 ? '' : 's'} for "${lastExecutedQuery}"`
      : 'Search across the open workspace'
  const activeFilterCount = [caseSensitive, wholeWord, useRegex, Boolean(includePattern.trim()), Boolean(excludePattern.trim())]
    .filter(Boolean)
    .length
  const allGroupsCollapsed = groupedResults.length > 0 && groupedResults.every((group) => collapsedGroups[group.path])

  const rememberSearch = useCallback((nextQuery: string) => {
    setSearchHistory((current) => {
      const nextHistory = touchSearchHistory(current, nextQuery)
      persistSearchHistory(nextHistory)
      return nextHistory
    })
  }, [])

  const runSearch = useCallback(async (nextQuery?: string) => {
    const normalizedQuery = (nextQuery ?? query).trim()
    if (!normalizedQuery || !rootPath) {
      setResults([])
      setLastExecutedQuery('')
      return
    }

    setSearching(true)
    try {
      const res = await window.electronAPI.searchFiles(normalizedQuery, rootPath, searchOptions)
      setResults(res)
      setLastExecutedQuery(normalizedQuery)
      setLastSearchedRevision(useWorkspaceStore.getState().revision)
      rememberSearch(normalizedQuery)
    } catch (err) {
      console.error(err)
      showErrorToast((err as Error).message || 'Search failed.', 'Search failed')
    } finally {
      setSearching(false)
    }
  }, [query, rememberSearch, rootPath, searchOptions])

  const handleReplaceAll = async () => {
    if (!query.trim() || !rootPath) return

    const confirmed = await confirmAction({
      title: 'Replace matches across workspace?',
      message: `Replace "${query}" with "${replaceQuery}" using the current search filters.`,
      confirmLabel: 'Replace All',
      cancelLabel: 'Cancel',
      tone: 'warning',
    })
    if (!confirmed) return
    
    setSearching(true)
    try {
      const result = await window.electronAPI.replaceInFiles(query, replaceQuery, rootPath, searchOptions)
      if (result.success) {
        showSuccessToast(`Updated ${result.count} file${result.count === 1 ? '' : 's'}.`, 'Replace complete')
        rememberSearch(query)
        await refreshTree()
        await runSearch(query)
      } else {
        showErrorToast(`Replace failed: ${result.error}`, 'Replace failed')
      }
    } catch (err) {
      console.error(err)
      showErrorToast((err as Error).message || 'Replace failed.', 'Replace failed')
    } finally {
      setSearching(false)
    }
  }

  const openMatch = useCallback(async (result: SearchResult) => {
    setPendingEditorTarget({ filePath: result.path, line: result.line })
    await openFile(result.path)
  }, [openFile, setPendingEditorTarget])

  const openAllMatches = useCallback(async () => {
    if (results.length === 0) return

    const uniquePaths = Array.from(new Set(results.map((result) => result.path)))
    const [firstPath, ...restPaths] = uniquePaths
    for (const filePath of restPaths) {
      await openFile(filePath)
    }

    const firstMatch = results.find((result) => result.path === firstPath)
    if (firstMatch) {
      setPendingEditorTarget({ filePath: firstMatch.path, line: firstMatch.line })
    }
    await openFile(firstPath)
    showInfoToast(`Opened ${uniquePaths.length} file${uniquePaths.length === 1 ? '' : 's'} with matches.`, 'Matches opened')
  }, [openFile, results, setPendingEditorTarget])

  useEffect(() => {
    setCollapsedGroups((current) => {
      const nextState = { ...current }
      const nextPaths = new Set(groupedResults.map((group) => group.path))
      let changed = false

      for (const group of groupedResults) {
        if (!(group.path in nextState)) {
          nextState[group.path] = false
          changed = true
        }
      }

      for (const path of Object.keys(nextState)) {
        if (!nextPaths.has(path)) {
          delete nextState[path]
          changed = true
        }
      }

      return changed ? nextState : current
    })
  }, [groupedResults])

  useEffect(() => {
    if (!rootPath) {
      setResults([])
      setLastExecutedQuery('')
      setLastSearchedRevision(null)
    }
  }, [rootPath])

  useEffect(() => {
    if (!rootPath || !lastExecutedQuery || activeSidebarView !== 'search' || lastSearchedRevision === revision) return

    const timeoutId = window.setTimeout(() => {
      void runSearch(lastExecutedQuery)
    }, 220)

    return () => window.clearTimeout(timeoutId)
  }, [activeSidebarView, lastExecutedQuery, lastSearchedRevision, revision, rootPath, runSearch])

  return (
    <div className="search-panel">
      <div className="sidebar-header">
        <span className="sidebar-title">SEARCH</span>
      </div>
      
      <div className="search-input-wrapper">
        <div className="search-toolbar-head">
          <div className="search-badge">
            <i className="fa-solid fa-folder-tree"></i>
            <span>Workspace</span>
          </div>
          <button 
            className={`search-toggle-btn ${showReplace ? 'active' : ''}`}
            onClick={() => setShowReplace(!showReplace)}
            title="Toggle Replace"
          >
            <i className="fa-solid fa-arrow-right-arrow-left"></i>
            <span>{showReplace ? 'Replace On' : 'Replace Off'}</span>
          </button>
        </div>

        <div className="search-filter-row">
          <button
            type="button"
            className={`search-filter-chip ${caseSensitive ? 'active' : ''}`}
            onClick={() => setCaseSensitive((value) => !value)}
          >
            Aa
          </button>
          <button
            type="button"
            className={`search-filter-chip ${wholeWord ? 'active' : ''}`}
            onClick={() => setWholeWord((value) => !value)}
          >
            Whole Word
          </button>
          <button
            type="button"
            className={`search-filter-chip ${useRegex ? 'active' : ''}`}
            onClick={() => setUseRegex((value) => !value)}
          >
            Regex
          </button>
          <span className="search-filter-summary">
            {activeFilterCount > 0 ? `${activeFilterCount} filters active` : 'Default search'}
          </span>
        </div>

        <label className="search-field">
          <span className="search-field-icon">
            <i className="fa-solid fa-magnifying-glass"></i>
          </span>
          <input 
            className="search-input"
            placeholder="Search in files..."
            value={query}
            onChange={e => {
              const nextQuery = e.target.value
              setQuery(nextQuery)
              if (!nextQuery.trim()) {
                setResults([])
                setLastExecutedQuery('')
              }
            }}
            onKeyDown={e => e.key === 'Enter' && void runSearch()}
          />
        </label>
        
        {showReplace && (
          <label className="search-field search-field-secondary">
            <span className="search-field-icon">
              <i className="fa-solid fa-wand-magic-sparkles"></i>
            </span>
            <input 
              className="search-input"
              placeholder="Replace with..."
              value={replaceQuery}
              onChange={e => setReplaceQuery(e.target.value)}
              onKeyDown={e => e.key === 'Enter' && void handleReplaceAll()}
            />
          </label>
        )}

        {searchHistory.length > 0 && (
          <div className="search-history">
            <span className="search-history-label">Recent</span>
            <div className="search-history-list">
              {searchHistory.map((item) => (
                <button
                  key={item}
                  type="button"
                  className="search-history-chip"
                  onClick={() => {
                    setQuery(item)
                    void runSearch(item)
                  }}
                >
                  <i className="fa-solid fa-clock-rotate-left"></i>
                  <span>{item}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        <div className="search-scope-grid">
          <label className="search-field search-field-secondary">
            <span className="search-field-icon">
              <i className="fa-solid fa-filter"></i>
            </span>
            <input
              className="search-input"
              placeholder="Include: src/**/*.ts, *.md"
              value={includePattern}
              onChange={(event) => setIncludePattern(event.target.value)}
            />
          </label>
          <label className="search-field search-field-secondary">
            <span className="search-field-icon">
              <i className="fa-solid fa-filter-circle-xmark"></i>
            </span>
            <input
              className="search-input"
              placeholder="Exclude: dist/**, *.log"
              value={excludePattern}
              onChange={(event) => setExcludePattern(event.target.value)}
            />
          </label>
        </div>

        <div className="search-actions">
          <button className="search-btn search-btn-primary" onClick={() => { void runSearch() }} disabled={searching}>
            <i className={`fa-solid ${searching ? 'fa-spinner fa-spin' : 'fa-bolt'}`}></i>
            <span>{searching ? 'Searching' : 'Find All'}</span>
          </button>
          {showReplace && (
            <button className="search-btn search-btn-warning" onClick={handleReplaceAll} disabled={searching || !query}>
              <i className="fa-solid fa-wand-magic-sparkles"></i>
              <span>Replace All</span>
            </button>
          )}
        </div>

        <div className="search-meta">
          <span>{resultLabel}</span>
          <div className="search-meta-actions">
            {groupedResults.length > 0 && (
              <>
                <button
                  type="button"
                  className="search-meta-action"
                  onClick={() => {
                    setCollapsedGroups(Object.fromEntries(groupedResults.map((group) => [group.path, !allGroupsCollapsed])))
                  }}
                >
                  {allGroupsCollapsed ? 'Expand All' : 'Collapse All'}
                </button>
                <button type="button" className="search-meta-action" onClick={() => { void openAllMatches() }}>
                  Open All
                </button>
              </>
            )}
            {results.length > 0 && (
              <button type="button" className="search-meta-action" onClick={() => showInfoToast(resultLabel, 'Search results')}>
                Summary
              </button>
            )}
          </div>
        </div>
      </div>

      <div className="search-results">
        {results.length === 0 ? (
          <div className="search-empty">
            <i className={`fa-solid ${searching ? 'fa-spinner fa-spin' : 'fa-magnifying-glass'}`}></i>
            <strong>{searching ? 'Searching workspace' : 'No results yet'}</strong>
            <span>
              {query.trim()
                ? 'Try a broader term, or toggle replace mode if you want to update matches.'
                : 'Enter a keyword to search across files in the current workspace.'}
            </span>
          </div>
        ) : (
          groupedResults.map((group) => {
            const isCollapsed = collapsedGroups[group.path]

            return (
              <div key={group.path} className="search-group">
                <button
                  type="button"
                  className={`search-group-head ${isCollapsed ? 'collapsed' : ''}`}
                  onClick={() => setCollapsedGroups((current) => ({ ...current, [group.path]: !current[group.path] }))}
                >
                  <span className="search-group-copy">
                    <span className="search-file-name">
                      <i className={`fa-solid ${isCollapsed ? 'fa-chevron-right' : 'fa-chevron-down'}`}></i>
                      <i className="fa-regular fa-file-lines"></i>
                      <span>{group.name}</span>
                    </span>
                    <span className="search-group-path">{group.relativePath}</span>
                  </span>
                  <span className="search-group-meta">
                    <span className="search-group-count">{group.matches.length}</span>
                    <span className="search-group-action">matches</span>
                  </span>
                </button>

                {!isCollapsed && (
                  <div className="search-group-body">
                    {group.matches.map((result, index) => {
                      const replacePreview = showReplace
                        ? buildReplacePreview(result.match, lastExecutedQuery || query, replaceQuery, searchOptions)
                        : ''

                      return (
                        <button
                          key={`${group.path}-${result.line}-${index}`}
                          type="button"
                          className="search-item"
                          onClick={() => { void openMatch(result) }}
                          title={`${group.relativePath}:${result.line}`}
                        >
                          <div className="search-file">
                            <span className="search-file-name">
                              <span>{group.name}</span>
                            </span>
                            <span className="search-line">line {result.line}</span>
                          </div>
                          <div className="search-match">{result.match}</div>
                          {replacePreview && (
                            <div className="search-replace-preview">
                              <span className="search-replace-label">Preview</span>
                              <span className="search-replace-value">{replacePreview}</span>
                            </div>
                          )}
                        </button>
                      )
                    })}
                  </div>
                )}
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}
