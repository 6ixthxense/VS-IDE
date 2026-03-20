import React, { Suspense, lazy } from 'react'
import { useEditorStore } from '../store/editorStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useUiStore } from '../store/uiStore'
import { Tabs } from './Tabs'
import { ErrorBoundary } from './ErrorBoundary'

const CodeEditorSurface = lazy(() =>
  import('./CodeEditorSurface').then((module) => ({ default: module.CodeEditorSurface }))
)
const MonacoEditorSurface = lazy(() =>
  import('./MonacoEditorSurface').then((module) => ({ default: module.MonacoEditorSurface }))
)

const Breadcrumbs = React.memo(({ parts }: { parts: string[] }) => (
  <div className="breadcrumbs">
    {parts.map((part, index) => (
      <React.Fragment key={index}>
        {index > 0 && <span className="breadcrumb-separator"><i className="fa-solid fa-chevron-right"></i></span>}
        <span className="breadcrumb-item">
          {index === parts.length - 1 ? (
            <i className="fa-regular fa-file-code" style={{ marginRight: '4px', fontSize: '12px' }}></i>
          ) : (
            <i className="fa-regular fa-folder" style={{ marginRight: '4px', fontSize: '12px' }}></i>
          )}
          {part}
        </span>
      </React.Fragment>
    ))}
  </div>
))

function EditorAttentionBanner({ groupId }: { groupId: string }) {
  const getActiveTab = useEditorStore((state) => state.getActiveTab)
  const dismissTabRecovery = useEditorStore((state) => state.dismissTabRecovery)
  const reloadTabFromDisk = useEditorStore((state) => state.reloadTabFromDisk)
  const saveTab = useEditorStore((state) => state.saveTab)
  const activeTab = getActiveTab(groupId)

  if (!activeTab?.recoveryState) return null

  const isConflict = activeTab.recoveryState === 'conflict'

  return (
    <div className={`editor-attention-banner ${isConflict ? 'conflict' : 'recovered'}`}>
      <div className="editor-attention-copy">
        <strong>{isConflict ? 'Disk conflict detected' : 'Recovered local edits'}</strong>
        <span>{activeTab.recoveryMessage}</span>
      </div>
      <div className="editor-attention-actions">
        <button type="button" onClick={() => dismissTabRecovery(activeTab.id)}>
          {isConflict ? 'Keep Local Draft' : 'Keep Editing'}
        </button>
        <button type="button" onClick={() => { void reloadTabFromDisk(activeTab.id) }}>
          Reload Disk
        </button>
        {isConflict && (
          <button type="button" className="primary" onClick={() => { void saveTab(activeTab.id) }}>
            Save Anyway
          </button>
        )}
      </div>
    </div>
  )
}

function EditorContent({ groupId }: { groupId: string }) {
  const getActiveTab = useEditorStore((s) => s.getActiveTab)
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const recentWorkspaces = useWorkspaceStore((s) => s.recentWorkspaces)
  const setRootPath = useWorkspaceStore((s) => s.setRootPath)
  const activeGroupId = useEditorStore((s) => s.activeGroupId)
  const splitGroup = useEditorStore((s) => s.splitGroup)
  const closeGroup = useEditorStore((s) => s.closeGroup)
  const groupsCount = useEditorStore((s) => s.groups.length)
  const showSidebarView = useUiStore((s) => s.showSidebarView)
  const activeTab = getActiveTab(groupId)

  if (!activeTab) {
    const workspaceName = rootPath?.replace(/\\/g, '/').split('/').pop() || 'VS-Monitor IDE'
    const hasWorkspace = Boolean(rootPath)

    return (
      <div
        className={`editor-pane ${activeGroupId === groupId ? 'active-group' : ''}`}
        onClick={() => useEditorStore.getState().setActiveGroupId(groupId)}
      >
        <Tabs groupId={groupId} />
        <div className="editor-welcome">
          <div className="welcome-content">
            <span className="welcome-eyebrow">{hasWorkspace ? 'Workspace Ready' : 'Start Here'}</span>
            <div className="welcome-logo">⌨️</div>
            <h2>{workspaceName}</h2>
            <p>
              {hasWorkspace
                ? 'Your workspace is open. Jump into a file, search the repo, or split the layout when you are ready.'
                : 'Open a folder to start browsing files, running code, and keeping an eye on the machine.'}
            </p>
            <div className="welcome-actions">
              {hasWorkspace ? (
                <>
                  <button
                    className="open-folder-btn welcome-cta"
                    onClick={() => useUiStore.setState({ showQuickOpen: true })}
                  >
                    <i className="fa-solid fa-file-magnifying-glass"></i> Quick Open
                  </button>
                  <button
                    className="welcome-secondary-btn"
                    onClick={() => showSidebarView('search')}
                  >
                    <i className="fa-solid fa-magnifying-glass"></i> Search in Files
                  </button>
                  <button
                    className="welcome-secondary-btn"
                    onClick={() => showSidebarView('explorer')}
                  >
                    <i className="fa-solid fa-folder-tree"></i> Show Explorer
                  </button>
                </>
              ) : (
                <>
                  <button
                    className="open-folder-btn welcome-cta"
                    onClick={() => useWorkspaceStore.getState().openFolder()}
                  >
                    <i className="fa-solid fa-folder-open"></i> Open Folder
                  </button>
                  <button
                    className="welcome-secondary-btn"
                    onClick={() => useUiStore.setState({ showCommandPalette: true })}
                  >
                    <i className="fa-solid fa-wand-magic-sparkles"></i> Command Palette
                  </button>
                </>
              )}
            </div>
            <div className="welcome-shortcuts">
              {hasWorkspace ? (
                <>
                  <div><kbd>Ctrl+P</kbd> Quick Open</div>
                  <div><kbd>Ctrl+Shift+P</kbd> Command Palette</div>
                  <div><kbd>Ctrl+`</kbd> Toggle Terminal</div>
                </>
              ) : (
                <>
                  <div><kbd>Ctrl+Shift+P</kbd> Command Palette</div>
                  <div><kbd>Ctrl+B</kbd> Toggle Sidebar</div>
                  <div><kbd>Ctrl+`</kbd> Toggle Terminal</div>
                </>
              )}
            </div>
            {!hasWorkspace && recentWorkspaces.length > 0 && (
              <div className="welcome-recents">
                <span className="welcome-recents-title">Recent Projects</span>
                <div className="welcome-recents-list">
                  {recentWorkspaces.slice(0, 4).map((workspacePath) => {
                    const label = workspacePath.replace(/\\/g, '/').split('/').pop() || workspacePath
                    return (
                      <button
                        key={workspacePath}
                        className="welcome-recent-btn"
                        onClick={() => {
                          void setRootPath(workspacePath)
                        }}
                      >
                        <strong>{label}</strong>
                        <span>{workspacePath}</span>
                      </button>
                    )
                  })}
                </div>
              </div>
            )}
          </div>
        </div>
      </div>
    )
  }

  const relativePath = rootPath && activeTab.path?.startsWith(rootPath)
    ? activeTab.path.slice(rootPath.length).replace(/^[\\/]/, '')
    : (activeTab.name || activeTab.path || '')

  const pathParts = relativePath.split(/[\\/]/)

  return (
    <div
      className={`editor-pane ${activeGroupId === groupId ? 'active-group' : ''}`}
      onClick={() => useEditorStore.getState().setActiveGroupId(groupId)}
    >
      <div className="editor-group-header">
        <Tabs groupId={groupId} />
        <div className="editor-group-actions">
          <button onClick={() => splitGroup(groupId, 'horizontal')} title="Split Editor">
            <i className="fa-solid fa-columns"></i>
          </button>
          {groupsCount > 1 && (
            <button onClick={() => closeGroup(groupId)} title="Close Split">
              <i className="fa-solid fa-xmark"></i>
            </button>
          )}
        </div>
      </div>
      <Breadcrumbs parts={pathParts} />
      <EditorAttentionBanner groupId={groupId} />
      <div className="editor-surface">
        <Suspense fallback={<div className="loading">Loading editor surface...</div>}>
          {activeTab.language === 'javascript' || activeTab.language === 'typescript' ? (
            <MonacoEditorSurface
              key={activeTab.path}
              filePath={activeTab.path}
              language={activeTab.language}
              initialContent={activeTab.content ?? ''}
            />
          ) : (
            <CodeEditorSurface
              key={activeTab.path}
              filePath={activeTab.path}
              language={activeTab.language}
              initialContent={activeTab.content ?? ''}
            />
          )}
        </Suspense>
      </div>
    </div>
  )
}

export function EditorPane() {
  const groups = useEditorStore(s => s.groups)
  const layoutDirection = useEditorStore((s) => s.layoutDirection)

  return (
    <ErrorBoundary fallbackMessage="Editor crashed — click Retry to reload">
      <div
        className="editor-splitter"
        style={{
          display: 'flex',
          flexDirection: layoutDirection === 'vertical' ? 'column' : 'row',
          height: '100%',
          width: '100%',
          gap: '1px',
          background: 'var(--bdr)',
        }}
      >
        {groups.map(group => (
          <EditorContent key={group.id} groupId={group.id} />
        ))}
      </div>
    </ErrorBoundary>
  )
}
