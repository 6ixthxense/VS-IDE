import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { EditorSelection, EditorState, type Extension } from '@codemirror/state'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView, highlightActiveLineGutter, lineNumbers } from '@codemirror/view'
import { useEditorStore } from '../store/editorStore'
import { useUiStore } from '../store/uiStore'
import { useConfigStore } from '../store/configStore'

async function loadLanguageExtensions(language: string): Promise<Extension[]> {
  switch (language) {
    case 'javascript': {
      const { javascript } = await import('@codemirror/lang-javascript')
      return [javascript({ jsx: true })]
    }
    case 'typescript': {
      const { javascript } = await import('@codemirror/lang-javascript')
      return [javascript({ jsx: true, typescript: true })]
    }
    case 'python': {
      const { python } = await import('@codemirror/lang-python')
      return [python()]
    }
    case 'json': {
      const { json } = await import('@codemirror/lang-json')
      return [json()]
    }
    case 'html': {
      const { html } = await import('@codemirror/lang-html')
      return [html()]
    }
    case 'css': {
      const { css } = await import('@codemirror/lang-css')
      return [css()]
    }
    case 'markdown': {
      const { markdown } = await import('@codemirror/lang-markdown')
      return [markdown()]
    }
    case 'yaml': {
      const { yaml } = await import('@codemirror/lang-yaml')
      return [yaml()]
    }
    default:
      return []
  }
}

const createEditorTheme = (fontSize: number, fontFamily: string) => EditorView.theme({
  '&': {
    height: '100%',
    fontSize: `${fontSize}px`,
    fontFamily: fontFamily,
    backgroundColor: 'var(--bg-panel)',
    color: 'var(--txt)',
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
    backgroundColor: 'color-mix(in srgb, var(--acc) 30%, transparent) !important',
  },
  '.cm-activeLine': {
    backgroundColor: 'var(--surface-1)',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'var(--surface-2)',
  },
  '.cm-gutters': {
    backgroundColor: 'var(--bg-sidebar)',
    color: 'var(--txt-dim)',
    border: 'none',
  },
  '.cm-lineNumbers .cm-gutterElement': {
    paddingLeft: '8px',
    paddingRight: '12px',
    minWidth: '40px',
  },
})

interface CodeEditorSurfaceProps {
  filePath: string
  language: string
  initialContent: string
}

export function CodeEditorSurface({ filePath, language, initialContent }: CodeEditorSurfaceProps) {
  const viewRef = useRef<EditorView | null>(null)
  const [languageExtensions, setLanguageExtensions] = useState<Extension[]>([])
  const [editorReadyKey, setEditorReadyKey] = useState(0)
  const setCursorPos = useUiStore((s) => s.setCursorPos)
  const pendingEditorTarget = useUiStore((s) => s.pendingEditorTarget)
  const clearPendingEditorTarget = useUiStore((s) => s.clearPendingEditorTarget)
  const saveTab = useEditorStore((s) => s.saveTab)
  const currentTab = useEditorStore((state) => state.tabs.find((tab) => tab.id === filePath))
  const {
    fontSize,
    fontFamily,
    theme,
    wordWrap,
    lineNumbers: showLineNumbers,
    autoSave,
    tabSize,
  } = useConfigStore()

  useEffect(() => {
    let cancelled = false

    void loadLanguageExtensions(language)
      .then((extensions) => {
        if (!cancelled) {
          setLanguageExtensions(extensions)
        }
      })
      .catch(() => {
        if (!cancelled) {
          setLanguageExtensions([])
        }
      })

    return () => {
      cancelled = true
    }
  }, [language])

  const extensions = useMemo(() => {
    const dynamicTheme = createEditorTheme(fontSize, fontFamily)
    return [
      dynamicTheme,
      EditorState.tabSize.of(tabSize),
      ...(wordWrap ? [EditorView.lineWrapping] : []),
      ...(showLineNumbers ? [lineNumbers(), highlightActiveLineGutter()] : []),
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
      ...languageExtensions,
    ]
  }, [filePath, fontFamily, fontSize, languageExtensions, setCursorPos, showLineNumbers, tabSize, wordWrap])

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

  useEffect(() => {
    if (!autoSave || !currentTab || currentTab.content === currentTab.savedContent) return

    const timeoutId = window.setTimeout(() => {
      void saveTab(filePath)
    }, 700)

    return () => window.clearTimeout(timeoutId)
  }, [autoSave, currentTab, filePath, saveTab])

  useEffect(() => {
    const view = viewRef.current
    if (!view || !pendingEditorTarget || pendingEditorTarget.filePath !== filePath) return

    const lineNumber = Math.max(1, Math.min(pendingEditorTarget.line, view.state.doc.lines))
    const line = view.state.doc.line(lineNumber)

    view.dispatch({
      selection: EditorSelection.cursor(line.from),
      scrollIntoView: true,
    })
    view.focus()
    clearPendingEditorTarget()
  }, [clearPendingEditorTarget, editorReadyKey, filePath, pendingEditorTarget])

  const handleCreateEditor = useCallback((view: EditorView) => {
    viewRef.current = view
    setEditorReadyKey((current) => current + 1)
  }, [])

  return (
    <CodeMirror
      value={initialContent}
      theme={theme === 'light' ? undefined : oneDark}
      extensions={extensions}
      onCreateEditor={handleCreateEditor}
      basicSetup={{
        lineNumbers: false,
        highlightActiveLineGutter: false,
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
      }}
      style={{
        height: '100%',
        overflow: 'hidden',
      }}
    />
  )
}
