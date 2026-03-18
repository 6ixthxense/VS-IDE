import React, { useCallback, useRef, useMemo, useEffect } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { javascript } from '@codemirror/lang-javascript'
import { python } from '@codemirror/lang-python'
import { json } from '@codemirror/lang-json'
import { html } from '@codemirror/lang-html'
import { css } from '@codemirror/lang-css'
import { markdown } from '@codemirror/lang-markdown'
import { yaml } from '@codemirror/lang-yaml'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView } from '@codemirror/view'
import { useEditorStore } from '../store/editorStore'
import { useUiStore } from '../store/uiStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useConfigStore } from '../store/configStore'
import { Tabs } from './Tabs'
import { ErrorBoundary } from './ErrorBoundary'

function getLanguageExtension(lang: string) {
  switch (lang) {
    case 'javascript': return javascript({ jsx: true })
    case 'typescript': return javascript({ jsx: true, typescript: true })
    case 'python': return python()
    case 'json': return json()
    case 'html': return html()
    case 'css': return css()
    case 'markdown': return markdown()
    case 'yaml': return yaml()
    case 'shell': return []
    default: return []
  }
}

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

/** Custom theme creator */
const createEditorTheme = (fontSize: number, fontFamily: string) => EditorView.theme({
  '&': {
    height: '100%',
    fontSize: `${fontSize}px`,
    fontFamily: fontFamily,
  },
  '.cm-scroller': {
    overflow: 'auto',
    fontFamily: fontFamily,
  },
  '.cm-content': {
    caretColor: 'var(--txt-bright)',
  },
  '.cm-cursor': {
    borderLeftColor: 'var(--txt-bright)',
    borderLeftWidth: '2px',
  },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
    backgroundColor: 'rgba(0, 122, 204, 0.3) !important',
  },
  '.cm-activeLine': {
    backgroundColor: 'rgba(255, 255, 255, 0.04)',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'rgba(255, 255, 255, 0.06)',
  },
  '.cm-gutters': {
    backgroundColor: 'transparent',
    color: 'var(--txt-dim)',
    border: 'none',
  },
  '.cm-lineNumbers .cm-gutterElement': {
    paddingLeft: '8px',
    paddingRight: '12px',
    minWidth: '40px',
  },
})

const CodeMirrorWrapper = React.memo(({
  filePath,
  language,
  initialContent,
}: {
  filePath: string
  language: string
  initialContent: string
}) => {
  const viewRef = useRef<EditorView | null>(null)
  const setCursorPos = useUiStore((s) => s.setCursorPos)
  const { fontSize, fontFamily } = useConfigStore()

  const langExt = useMemo(() => getLanguageExtension(language), [language])

  const extensions = useMemo(() => {
    const dynamicTheme = createEditorTheme(fontSize, fontFamily)
    const exts = [
      dynamicTheme,
      EditorView.lineWrapping,
      EditorView.updateListener.of((update) => {
        if (update.selectionSet) {
          const pos = update.state.selection.main.head
          const line = update.state.doc.lineAt(pos)
          setCursorPos({
            line: line.number,
            col: pos - line.from + 1,
          })
        }
        if (update.docChanged) {
          const value = update.state.doc.toString()
          useEditorStore.getState().setContent(filePath, value)
        }
      }),
    ]
    if (Array.isArray(langExt)) {
      exts.push(...langExt)
    } else {
      exts.push(langExt)
    }
    return exts
  }, [langExt, filePath, setCursorPos, fontSize, fontFamily])

  // Ctrl+S save
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        if (viewRef.current) {
          const value = viewRef.current.state.doc.toString()
          useEditorStore.getState().setContent(filePath, value)
        }
        void useEditorStore.getState().saveTab(filePath)
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [filePath])

  const handleCreateEditor = useCallback((view: EditorView) => {
    viewRef.current = view
  }, [])

  return (
    <CodeMirror
      value={initialContent}
      theme={oneDark}
      extensions={extensions}
      onCreateEditor={handleCreateEditor}
      basicSetup={{
        lineNumbers: true,
        highlightActiveLineGutter: true,
        highlightActiveLine: true,
        foldGutter: true,
        dropCursor: true,
        allowMultipleSelections: true,
        indentOnInput: true,
        bracketMatching: true,
        closeBrackets: true,
        autocompletion: true,
        crosshairCursor: false,
        rectangularSelection: true,
        highlightSelectionMatches: true,
        tabSize: 4,
      }}
      style={{
        height: '100%',
        overflow: 'hidden',
      }}
    />
  )
})

function EditorContent({ groupId }: { groupId: string }) {
  const getActiveTab = useEditorStore((s) => s.getActiveTab)
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const activeGroupId = useEditorStore((s) => s.activeGroupId)
  const splitGroup = useEditorStore((s) => s.splitGroup)
  const activeTab = getActiveTab(groupId)

  if (!activeTab) {
    if (groupId !== 'main') return null

    return (
      <div
        className={`editor-pane ${activeGroupId === groupId ? 'active-group' : ''}`}
        onClick={() => useEditorStore.getState().setActiveGroupId(groupId)}
      >
        <Tabs groupId={groupId} />
        <div className="editor-welcome">
          <div className="welcome-content">
            <div className="welcome-logo">⌨️</div>
            <h2>VS-Monitor IDE</h2>
            <p>Open a folder to get started</p>
            <button
              className="open-folder-btn welcome-cta"
              onClick={() => useWorkspaceStore.getState().openFolder()}
              style={{ marginBottom: '24px', padding: '10px 24px', fontSize: '14px' }}
            >
              <i className="fa-solid fa-folder-open"></i> Open Folder
            </button>
            <div className="welcome-shortcuts">
              <div><kbd>Ctrl+Shift+P</kbd> Command Palette</div>
              <div><kbd>Ctrl+S</kbd> Save</div>
              <div><kbd>Ctrl+`</kbd> Toggle Terminal</div>
            </div>
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
        </div>
      </div>
      <Breadcrumbs parts={pathParts} />
      <div className="editor-surface">
        <CodeMirrorWrapper
          key={activeTab.path}
          filePath={activeTab.path}
          language={activeTab.language}
          initialContent={activeTab.content ?? ''}
        />
      </div>
    </div>
  )
}

export function EditorPane() {
  const groups = useEditorStore(s => s.groups)

  return (
    <ErrorBoundary fallbackMessage="Editor crashed — click Retry to reload">
      <div className="editor-splitter" style={{ display: 'flex', height: '100%', width: '100%', gap: '1px', background: 'var(--bdr)' }}>
        {groups.map(group => (
          <EditorContent key={group.id} groupId={group.id} />
        ))}
      </div>
    </ErrorBoundary>
  )
}
