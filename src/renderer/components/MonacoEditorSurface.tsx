import React, { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import Editor, { loader, type OnMount } from '@monaco-editor/react'
import * as monaco from 'monaco-editor'
import type { ProblemEntry } from '@shared/types/ipc'
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import { useEditorStore } from '../store/editorStore'
import { useProblemsStore } from '../store/problemsStore'
import { useUiStore } from '../store/uiStore'
import { useConfigStore } from '../store/configStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { showErrorToast, showInfoToast, showSuccessToast } from '../store/feedbackStore'
import { applyTextSpanEdits, type TextSpanEdit } from '../utils/textEdits'

const MONACO_SUPPORTED_LANGUAGES = new Set(['javascript', 'typescript'])
const WORKSPACE_CODE_EXTENSIONS = ['.ts', '.tsx', '.js', '.jsx', '.mts', '.cts', '.mjs', '.cjs', '.d.ts']
const MAX_PRELOADED_WORKSPACE_MODELS = 250
const MAX_PRELOADED_MODEL_CHARACTERS = 200_000
const workspaceSyncPromises = new Map<string, Promise<void>>()
const configuredWorkspaces = new Set<string>()
let loaderConfigured = false
let diagnosticsBridgeConfigured = false

interface WorkerTextSpan {
  start: number
  length: number
}

interface WorkerDefinitionEntry {
  fileName: string
  textSpan: WorkerTextSpan
  name?: string
}

interface WorkerReferenceEntry {
  fileName: string
  textSpan: WorkerTextSpan
  isDefinition?: boolean
  isWriteAccess?: boolean
}

interface WorkerRenameInfo {
  canRename: boolean
  displayName?: string
  localizedErrorMessage?: string
}

interface SymbolLocationItem {
  id: string
  filePath: string
  line: number
  column: number
  preview: string
  isDefinition?: boolean
  isWriteAccess?: boolean
}

interface SymbolResultsDialogState {
  title: string
  subtitle: string
  items: SymbolLocationItem[]
}

interface RenameDialogState {
  fileName: string
  position: number
  symbolName: string
  nextName: string
  pending: boolean
}

function normalizePath(value: string) {
  return value.replace(/\\/g, '/')
}

function inferMonacoLanguage(filePath: string, fallbackLanguage: string) {
  const normalizedPath = normalizePath(filePath).toLowerCase()

  if (normalizedPath.endsWith('.ts') || normalizedPath.endsWith('.tsx') || normalizedPath.endsWith('.mts') || normalizedPath.endsWith('.cts') || normalizedPath.endsWith('.d.ts')) {
    return 'typescript'
  }

  if (normalizedPath.endsWith('.js') || normalizedPath.endsWith('.jsx') || normalizedPath.endsWith('.mjs') || normalizedPath.endsWith('.cjs')) {
    return 'javascript'
  }

  return MONACO_SUPPORTED_LANGUAGES.has(fallbackLanguage) ? fallbackLanguage : 'javascript'
}

function supportsMonacoLanguage(language: string) {
  return MONACO_SUPPORTED_LANGUAGES.has(language)
}

function filePathToModelPath(filePath: string) {
  return monaco.Uri.file(filePath).toString()
}

function shouldPreloadWorkspaceFile(filePath: string) {
  const normalizedPath = normalizePath(filePath).toLowerCase()
  return WORKSPACE_CODE_EXTENSIONS.some((extension) => normalizedPath.endsWith(extension))
}

function ensureMonacoLoader() {
  if (loaderConfigured) return

  globalThis.MonacoEnvironment = {
    getWorker(_moduleId, label) {
      if (label === 'typescript' || label === 'javascript') {
        return new tsWorker()
      }

      return new editorWorker()
    },
  }

  loader.config({ monaco })
  monaco.languages.typescript.typescriptDefaults.setEagerModelSync(true)
  monaco.languages.typescript.javascriptDefaults.setEagerModelSync(true)
  ensureMonacoDiagnosticsBridge()
  loaderConfigured = true
}

function resolveModuleKind(value: unknown) {
  const kinds = monaco.languages.typescript.ModuleKind as Record<string, number>
  switch (String(value ?? '').toLowerCase()) {
    case 'commonjs':
      return kinds.CommonJS
    case 'amd':
      return kinds.AMD
    case 'system':
      return kinds.System
    case 'umd':
      return kinds.UMD
    case 'nodenext':
      return kinds.NodeNext ?? kinds.ESNext
    case 'es2015':
    case 'es6':
      return kinds.ES2015
    case 'es2020':
      return kinds.ES2020 ?? kinds.ESNext
    case 'es2022':
      return kinds.ES2022 ?? kinds.ESNext
    case 'preserve':
      return kinds.Preserve ?? kinds.ESNext
    case 'esnext':
    default:
      return kinds.ESNext
  }
}

function resolveModuleResolution(value: unknown) {
  const kinds = monaco.languages.typescript.ModuleResolutionKind as Record<string, number>
  switch (String(value ?? '').toLowerCase()) {
    case 'classic':
      return kinds.Classic
    case 'node16':
      return kinds.Node16 ?? kinds.NodeJs
    case 'nodenext':
      return kinds.NodeNext ?? kinds.NodeJs
    case 'bundler':
      return kinds.Bundler ?? kinds.NodeJs
    case 'node':
    case 'node10':
    case 'nodejs':
    default:
      return kinds.NodeJs
  }
}

function resolveScriptTarget(value: unknown) {
  const targets = monaco.languages.typescript.ScriptTarget as Record<string, number>
  switch (String(value ?? '').toLowerCase()) {
    case 'es5':
      return targets.ES5
    case 'es2015':
    case 'es6':
      return targets.ES2015
    case 'es2016':
      return targets.ES2016
    case 'es2017':
      return targets.ES2017
    case 'es2018':
      return targets.ES2018
    case 'es2019':
      return targets.ES2019
    case 'es2020':
      return targets.ES2020
    case 'es2021':
      return targets.ES2021 ?? targets.ES2020
    case 'es2022':
      return targets.ES2022 ?? targets.ES2020
    case 'latest':
    case 'esnext':
    default:
      return targets.Latest
  }
}

function resolveJsxEmit(value: unknown) {
  const jsx = monaco.languages.typescript.JsxEmit as Record<string, number>
  switch (String(value ?? '').toLowerCase()) {
    case 'preserve':
      return jsx.Preserve
    case 'react':
      return jsx.React
    case 'reactnative':
      return jsx.ReactNative
    case 'react-jsxdev':
      return jsx.ReactJSXDev ?? jsx.React
    case 'react-jsx':
    default:
      return jsx.ReactJSX ?? jsx.React
  }
}

function applyWorkspaceCompilerOptions(rootPath: string, compilerOptions: Record<string, unknown>) {
  const nextOptions = {
    allowJs: compilerOptions.allowJs !== false,
    allowNonTsExtensions: true,
    allowSyntheticDefaultImports: compilerOptions.allowSyntheticDefaultImports !== false,
    esModuleInterop: compilerOptions.esModuleInterop !== false,
    module: resolveModuleKind(compilerOptions.module),
    moduleResolution: resolveModuleResolution(compilerOptions.moduleResolution),
    jsx: resolveJsxEmit(compilerOptions.jsx),
    target: resolveScriptTarget(compilerOptions.target),
    strict: Boolean(compilerOptions.strict),
    resolveJsonModule: compilerOptions.resolveJsonModule !== false,
    checkJs: Boolean(compilerOptions.checkJs),
    baseUrl: typeof compilerOptions.baseUrl === 'string' ? compilerOptions.baseUrl : '.',
    paths: typeof compilerOptions.paths === 'object' && compilerOptions.paths !== null
      ? compilerOptions.paths as Record<string, string[]>
      : undefined,
    rootDirs: [normalizePath(rootPath)],
  }

  monaco.languages.typescript.typescriptDefaults.setCompilerOptions(nextOptions)
  monaco.languages.typescript.javascriptDefaults.setCompilerOptions(nextOptions)
}

async function configureWorkspaceCompilerOptions(rootPath: string) {
  if (configuredWorkspaces.has(rootPath)) {
    return
  }

  const candidates = ['tsconfig.json', 'jsconfig.json']
  let compilerOptions: Record<string, unknown> = {}

  for (const candidate of candidates) {
    const configPath = `${rootPath}${window.electronAPI.isWindows ? '\\' : '/'}${candidate}`
    const exists = await window.electronAPI.pathExists(configPath)
    if (!exists) {
      continue
    }

    try {
      const raw = await window.electronAPI.readFile(configPath)
      const parsed = JSON.parse(raw) as { compilerOptions?: Record<string, unknown> }
      compilerOptions = parsed.compilerOptions ?? {}
      break
    } catch {
      // Fall back to defaults if the workspace config is not plain JSON.
    }
  }

  applyWorkspaceCompilerOptions(rootPath, compilerOptions)
  configuredWorkspaces.add(rootPath)
}

function ensureWorkspaceModel(
  filePath: string,
  language: string,
  content: string,
  options: { overwriteContent?: boolean } = {}
) {
  const uri = monaco.Uri.file(filePath)
  const nextLanguage = inferMonacoLanguage(filePath, language)
  const existingModel = monaco.editor.getModel(uri)

  if (existingModel) {
    if (existingModel.getLanguageId() !== nextLanguage) {
      monaco.editor.setModelLanguage(existingModel, nextLanguage)
    }
    if (options.overwriteContent !== false && existingModel.getValue() !== content) {
      existingModel.setValue(content)
    }
    return existingModel
  }

  return monaco.editor.createModel(content, nextLanguage, uri)
}

async function preloadWorkspaceModels(rootPath: string, activeFilePath: string) {
  if (workspaceSyncPromises.has(rootPath)) {
    return workspaceSyncPromises.get(rootPath)!
  }

  const promise = (async () => {
    await configureWorkspaceCompilerOptions(rootPath)
    const filePaths = await window.electronAPI.getAllFiles(rootPath)
    const openTabs = useEditorStore.getState().tabs
      .filter((tab) => tab.path.startsWith(rootPath) && supportsMonacoLanguage(tab.language))
    const openTabPaths = openTabs
      .filter((tab) => tab.path.startsWith(rootPath) && tab.path !== activeFilePath && supportsMonacoLanguage(tab.language))
      .map((tab) => tab.path)
    const nextPaths = Array.from(new Set([
      ...openTabPaths,
      ...filePaths.filter((filePath) => shouldPreloadWorkspaceFile(filePath) && filePath !== activeFilePath),
    ])).slice(0, MAX_PRELOADED_WORKSPACE_MODELS)

    for (const filePath of nextPaths) {
      const uri = monaco.Uri.file(filePath)
      if (monaco.editor.getModel(uri)) {
        continue
      }

      try {
        const openTab = openTabs.find((tab) => tab.path === filePath)
        const content = openTab?.content ?? await window.electronAPI.readFile(filePath)
        if (content.length > MAX_PRELOADED_MODEL_CHARACTERS) {
          continue
        }
        ensureWorkspaceModel(filePath, openTab?.language ?? inferMonacoLanguage(filePath, 'typescript'), content, {
          overwriteContent: false,
        })
      } catch {
        // Ignore files that disappear while models are warming up.
      }
    }
  })().finally(() => {
    workspaceSyncPromises.delete(rootPath)
  })

  workspaceSyncPromises.set(rootPath, promise)
  return promise
}

function resolveMarkerCode(marker: monaco.editor.IMarker) {
  if (typeof marker.code === 'string') {
    return marker.code
  }

  if (
    marker.code
    && typeof marker.code === 'object'
    && 'value' in marker.code
    && typeof marker.code.value === 'string'
  ) {
    return marker.code.value
  }

  return undefined
}

function toProblemSeverity(markerSeverity: monaco.MarkerSeverity): ProblemEntry['severity'] | null {
  if (markerSeverity === monaco.MarkerSeverity.Error) {
    return 'error'
  }

  if (markerSeverity === monaco.MarkerSeverity.Warning) {
    return 'warning'
  }

  return null
}

function collectLiveTypeScriptProblems(rootPath: string) {
  const normalizedRoot = normalizePath(rootPath).toLowerCase()

  return monaco.editor
    .getModels()
    .filter((model) => {
      if (model.uri.scheme !== 'file' || !supportsMonacoLanguage(model.getLanguageId())) {
        return false
      }

      const normalizedPath = normalizePath(model.uri.fsPath).toLowerCase()
      return normalizedPath === normalizedRoot || normalizedPath.startsWith(`${normalizedRoot}/`)
    })
    .flatMap<ProblemEntry>((model) =>
      monaco.editor
        .getModelMarkers({ resource: model.uri })
        .flatMap((marker) => {
          const severity = toProblemSeverity(marker.severity)
          if (!severity) {
            return []
          }

          const code = resolveMarkerCode(marker)
          const filePath = model.uri.fsPath
          const message = marker.message.trim()

          return [{
            id: [
              'monaco',
              filePath,
              marker.startLineNumber,
              marker.startColumn,
              severity,
              code ?? '',
              message,
            ].join(':'),
            filePath,
            line: marker.startLineNumber,
            column: marker.startColumn,
            message,
            source: 'typescript',
            severity,
            code,
          }]
        })
    )
}

function syncLiveTypeScriptProblems() {
  const rootPath = useWorkspaceStore.getState().rootPath
  if (!rootPath) {
    return
  }

  useProblemsStore.getState().setLiveEntries(rootPath, collectLiveTypeScriptProblems(rootPath))
}

function ensureMonacoDiagnosticsBridge() {
  if (diagnosticsBridgeConfigured) {
    return
  }

  monaco.editor.onDidChangeMarkers(() => {
    syncLiveTypeScriptProblems()
  })

  diagnosticsBridgeConfigured = true
}

async function getTypeScriptWorker(
  model: monaco.editor.ITextModel,
  language: string
): Promise<monaco.languages.typescript.TypeScriptWorker> {
  const factory = language === 'typescript'
    ? await monaco.languages.typescript.getTypeScriptWorker()
    : await monaco.languages.typescript.getJavaScriptWorker()

  return factory(model.uri)
}

function workerFileNameToFilePath(fileName: string) {
  const uri = monaco.Uri.parse(fileName)
  return uri.scheme === 'file' ? uri.fsPath : fileName
}

async function ensureWorkerFileModel(fileName: string, fallbackLanguage: string) {
  const uri = monaco.Uri.parse(fileName)
  const existingModel = monaco.editor.getModel(uri)
  if (existingModel) {
    return existingModel
  }

  if (uri.scheme !== 'file') {
    return null
  }

  try {
    const content = await window.electronAPI.readFile(uri.fsPath)
    return ensureWorkspaceModel(uri.fsPath, inferMonacoLanguage(uri.fsPath, fallbackLanguage), content, {
      overwriteContent: false,
    })
  } catch {
    return null
  }
}

function getSymbolPreview(model: monaco.editor.ITextModel, lineNumber: number) {
  const lineContent = model.getLineContent(lineNumber).trim()
  return lineContent || '(blank line)'
}

async function toSymbolLocationItem(
  entry: WorkerDefinitionEntry | WorkerReferenceEntry,
  fallbackLanguage: string
): Promise<SymbolLocationItem | null> {
  const model = await ensureWorkerFileModel(entry.fileName, fallbackLanguage)
  if (!model) {
    return null
  }

  const position = model.getPositionAt(entry.textSpan.start)
  const filePath = workerFileNameToFilePath(entry.fileName)

  return {
    id: [
      filePath,
      entry.textSpan.start,
      entry.textSpan.length,
      'isDefinition' in entry && entry.isDefinition ? 'definition' : 'reference',
      'isWriteAccess' in entry && entry.isWriteAccess ? 'write' : 'read',
    ].join(':'),
    filePath,
    line: position.lineNumber,
    column: position.column,
    preview: getSymbolPreview(model, position.lineNumber),
    isDefinition: 'isDefinition' in entry ? entry.isDefinition : undefined,
    isWriteAccess: 'isWriteAccess' in entry ? entry.isWriteAccess : undefined,
  }
}

function sortSymbolLocations(items: SymbolLocationItem[]) {
  return [...items].sort((left, right) =>
    Number(Boolean(right.isDefinition)) - Number(Boolean(left.isDefinition))
    || left.filePath.localeCompare(right.filePath)
    || left.line - right.line
    || left.column - right.column
  )
}

function applyTheme(theme: string) {
  if (theme === 'light') {
    monaco.editor.setTheme('vs')
    return
  }

  if (theme === 'amoled') {
    monaco.editor.defineTheme('vs-monitor-amoled', {
      base: 'vs-dark',
      inherit: true,
      rules: [],
      colors: {
        'editor.background': '#000000',
        'editor.lineHighlightBackground': '#0a0a0a',
      },
    })
    monaco.editor.setTheme('vs-monitor-amoled')
    return
  }

  monaco.editor.setTheme('vs-dark')
}

interface MonacoEditorSurfaceProps {
  filePath: string
  language: string
  initialContent: string
}

export function MonacoEditorSurface({ filePath, language, initialContent }: MonacoEditorSurfaceProps) {
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const renameInputRef = useRef<HTMLInputElement | null>(null)
  const editorDisposablesRef = useRef<monaco.IDisposable[]>([])
  const setCursorPos = useUiStore((state) => state.setCursorPos)
  const pendingEditorTarget = useUiStore((state) => state.pendingEditorTarget)
  const clearPendingEditorTarget = useUiStore((state) => state.clearPendingEditorTarget)
  const openFile = useEditorStore((state) => state.openFile)
  const saveTab = useEditorStore((state) => state.saveTab)
  const currentTab = useEditorStore((state) => state.tabs.find((tab) => tab.id === filePath))
  const rootPath = useWorkspaceStore((state) => state.rootPath)
  const revision = useWorkspaceStore((state) => state.revision)
  const [resultsDialog, setResultsDialog] = useState<SymbolResultsDialogState | null>(null)
  const [renameDialog, setRenameDialog] = useState<RenameDialogState | null>(null)
  const {
    fontSize,
    fontFamily,
    theme,
    wordWrap,
    lineNumbers: showLineNumbers,
    autoSave,
    tabSize,
  } = useConfigStore()

  const value = currentTab?.content ?? initialContent
  const path = useMemo(() => filePathToModelPath(filePath), [filePath])
  const modelLanguage = useMemo(() => inferMonacoLanguage(filePath, language), [filePath, language])

  useEffect(() => {
    ensureMonacoLoader()
    ensureWorkspaceModel(filePath, language, value, { overwriteContent: true })
  }, [filePath, language, value])

  useEffect(() => {
    if (!rootPath || !supportsMonacoLanguage(language)) return
    void preloadWorkspaceModels(rootPath, filePath)
  }, [filePath, language, revision, rootPath])

  useEffect(() => {
    if (!rootPath || !supportsMonacoLanguage(language)) {
      return
    }

    const timeoutId = window.setTimeout(() => {
      syncLiveTypeScriptProblems()
    }, 0)

    return () => window.clearTimeout(timeoutId)
  }, [filePath, language, rootPath, value])

  useEffect(() => {
    applyTheme(theme)
  }, [theme])

  useEffect(() => {
    if (!renameDialog) {
      return
    }

    const timeoutId = window.setTimeout(() => {
      renameInputRef.current?.focus()
      renameInputRef.current?.select()
    }, 0)

    return () => window.clearTimeout(timeoutId)
  }, [renameDialog])

  useEffect(() => {
    if (!resultsDialog && !renameDialog) {
      return
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') {
        return
      }

      if (renameDialog?.pending) {
        return
      }

      if (renameDialog) {
        setRenameDialog(null)
        return
      }

      if (resultsDialog) {
        setResultsDialog(null)
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [renameDialog, resultsDialog])

  useEffect(() => () => {
    for (const disposable of editorDisposablesRef.current) {
      disposable.dispose()
    }
    editorDisposablesRef.current = []
  }, [])

  useEffect(() => {
    const editor = editorRef.current
    if (!editor || !pendingEditorTarget || pendingEditorTarget.filePath !== filePath) return

    const lineNumber = Math.max(
      1,
      Math.min(pendingEditorTarget.line, editor.getModel()?.getLineCount() ?? pendingEditorTarget.line)
    )
    const column = Math.max(1, pendingEditorTarget.column ?? 1)
    editor.setPosition({
      lineNumber,
      column,
    })
    editor.revealLineInCenter(lineNumber)
    editor.focus()
    clearPendingEditorTarget()
  }, [clearPendingEditorTarget, filePath, pendingEditorTarget])

  const navigateToSymbolLocation = async (target: SymbolLocationItem) => {
    setPendingEditorTarget({
      filePath: target.filePath,
      line: target.line,
      column: target.column,
    })
    setResultsDialog(null)
    await openFile(target.filePath)
  }

  const getActiveWorkerContext = async () => {
    const editor = editorRef.current
    const model = editor?.getModel()
    const position = editor?.getPosition()
    if (!editor || !model || !position) {
      return null
    }

    if (rootPath) {
      await preloadWorkspaceModels(rootPath, filePath)
    }

    const worker = await getTypeScriptWorker(model, language)
    const offset = model.getOffsetAt(position)
    const currentWord = model.getWordAtPosition(position)?.word ?? 'symbol'

    return {
      editor,
      model,
      worker,
      offset,
      currentWord,
    }
  }

  const goToDefinition = async () => {
    const context = await getActiveWorkerContext()
    if (!context) {
      return
    }

    const definitions = await context.worker.getDefinitionAtPosition(
      context.model.uri.toString(),
      context.offset
    ) as ReadonlyArray<WorkerDefinitionEntry> | undefined

    if (!definitions || definitions.length === 0) {
      showInfoToast('No definition was found for the current symbol.', 'Definition unavailable')
      return
    }

    const items = (await Promise.all(
      definitions.map((entry) => toSymbolLocationItem(entry, language))
    )).filter((entry): entry is SymbolLocationItem => Boolean(entry))

    if (items.length === 0) {
      showInfoToast('The definition target could not be opened from the workspace models.', 'Definition unavailable')
      return
    }

    if (items.length === 1) {
      await navigateToSymbolLocation(items[0])
      return
    }

    setResultsDialog({
      title: `Definitions: ${context.currentWord}`,
      subtitle: 'Multiple definition targets were found for this symbol.',
      items: sortSymbolLocations(items),
    })
  }

  const showReferences = async () => {
    const context = await getActiveWorkerContext()
    if (!context) {
      return
    }

    const references = await context.worker.getReferencesAtPosition(
      context.model.uri.toString(),
      context.offset
    ) as WorkerReferenceEntry[] | undefined

    if (!references || references.length === 0) {
      showInfoToast('No references were found for the current symbol.', 'References unavailable')
      return
    }

    const items = (await Promise.all(
      references.map((entry) => toSymbolLocationItem(entry, language))
    )).filter((entry): entry is SymbolLocationItem => Boolean(entry))

    if (items.length === 0) {
      showInfoToast('The references could not be resolved from the loaded workspace files.', 'References unavailable')
      return
    }

    setResultsDialog({
      title: `References: ${context.currentWord}`,
      subtitle: 'Results reflect the open and warmed TypeScript workspace models.',
      items: sortSymbolLocations(items),
    })
  }

  const openRename = async () => {
    const context = await getActiveWorkerContext()
    if (!context) {
      return
    }

    const renameInfo = await context.worker.getRenameInfo(
      context.model.uri.toString(),
      context.offset,
      { allowRenameOfImportPath: false }
    ) as WorkerRenameInfo

    if (!renameInfo?.canRename) {
      showInfoToast(
        renameInfo?.localizedErrorMessage || 'This symbol cannot be renamed here.',
        'Rename unavailable'
      )
      return
    }

    const symbolName = renameInfo.displayName || context.currentWord
    setRenameDialog({
      fileName: context.model.uri.toString(),
      position: context.offset,
      symbolName,
      nextName: symbolName,
      pending: false,
    })
  }

  const applyRename = async () => {
    if (!renameDialog) {
      return
    }

    const nextName = renameDialog.nextName.trim()
    if (!nextName) {
      showInfoToast('Enter a new symbol name before applying rename.', 'Rename unavailable')
      return
    }

    if (nextName === renameDialog.symbolName) {
      setRenameDialog(null)
      return
    }

    setRenameDialog((current) => current ? { ...current, pending: true } : current)

    try {
      const sourceModel = await ensureWorkerFileModel(renameDialog.fileName, language)
      if (!sourceModel) {
        throw new Error('The active model is no longer available for rename.')
      }

      if (rootPath) {
        await preloadWorkspaceModels(rootPath, filePath)
      }

      const worker = await getTypeScriptWorker(sourceModel, sourceModel.getLanguageId())
      const renameInfo = await worker.getRenameInfo(
        renameDialog.fileName,
        renameDialog.position,
        { allowRenameOfImportPath: false }
      ) as WorkerRenameInfo

      if (!renameInfo?.canRename) {
        throw new Error(renameInfo?.localizedErrorMessage || 'This symbol cannot be renamed here.')
      }

      const locations = await worker.findRenameLocations(
        renameDialog.fileName,
        renameDialog.position,
        false,
        false,
        false
      ) as readonly WorkerReferenceEntry[] | undefined

      if (!locations || locations.length === 0) {
        throw new Error('No rename locations were returned for this symbol.')
      }

      const editsByFile = new Map<string, TextSpanEdit[]>()
      for (const location of locations) {
        const fileName = location.fileName
        const nextEdits = editsByFile.get(fileName) ?? []
        nextEdits.push({
          start: location.textSpan.start,
          length: location.textSpan.length,
          newText: nextName,
        })
        editsByFile.set(fileName, nextEdits)
      }

      let touchedFileCount = 0

      for (const [workerFileName, edits] of editsByFile) {
        const targetFilePath = workerFileNameToFilePath(workerFileName)
        const targetUri = monaco.Uri.parse(workerFileName)
        const targetModel = monaco.editor.getModel(targetUri)
        const openTab = useEditorStore.getState().tabs.find((tab) => tab.path === targetFilePath)
        const baseContent = targetModel?.getValue()
          ?? openTab?.content
          ?? await window.electronAPI.readFile(targetFilePath)
        const nextContent = applyTextSpanEdits(baseContent, edits)

        if (targetModel && targetModel.getValue() !== nextContent) {
          targetModel.setValue(nextContent)
        }

        if (openTab) {
          useEditorStore.getState().setContent(targetFilePath, nextContent)
          const saved = await useEditorStore.getState().saveTab(targetFilePath)
          if (!saved) {
            throw new Error(`Unable to save the renamed result for ${openTab.name}.`)
          }
        } else {
          const result = await window.electronAPI.writeFile(targetFilePath, nextContent)
          if (!result.success) {
            throw new Error(result.error || `Unable to write ${targetFilePath}.`)
          }
        }

        touchedFileCount += 1
      }

      const sourcePosition = sourceModel.getPositionAt(renameDialog.position)
      editorRef.current?.setPosition(sourcePosition)
      editorRef.current?.revealPositionInCenter(sourcePosition)
      editorRef.current?.focus()

      setRenameDialog(null)
      syncLiveTypeScriptProblems()
      showSuccessToast(
        `Renamed "${renameDialog.symbolName}" to "${nextName}" in ${touchedFileCount} file${touchedFileCount === 1 ? '' : 's'}.`,
        'Rename complete'
      )
    } catch (error) {
      const message = (error as Error).message || 'Unable to rename the selected symbol.'
      setRenameDialog((current) => current ? { ...current, pending: false } : current)
      showErrorToast(message, 'Rename failed')
    }
  }

  useEffect(() => {
    if (!autoSave || !currentTab || currentTab.content === currentTab.savedContent) return

    const timeoutId = window.setTimeout(() => {
      void saveTab(filePath)
    }, 700)

    return () => window.clearTimeout(timeoutId)
  }, [autoSave, currentTab, filePath, saveTab])

  const handleEditorChange = (nextValue: string | undefined) => {
    useEditorStore.getState().setContent(filePath, nextValue ?? '')
  }

  const handleEditorMount: OnMount = (editor) => {
    for (const disposable of editorDisposablesRef.current) {
      disposable.dispose()
    }
    editorDisposablesRef.current = []
    editorRef.current = editor

    const model = editor.getModel()
    if (model) {
      ensureWorkspaceModel(filePath, language, model.getValue())
    }

    editorDisposablesRef.current.push(editor.onDidChangeCursorPosition((event) => {
      setCursorPos({
        line: event.position.lineNumber,
        col: event.position.column,
      })
    }))

    editorDisposablesRef.current.push(editor.onKeyDown((event) => {
      if (event.keyCode === monaco.KeyCode.F12 && event.shiftKey) {
        event.preventDefault()
        event.stopPropagation()
        void showReferences()
        return
      }

      if (event.keyCode === monaco.KeyCode.F12) {
        event.preventDefault()
        event.stopPropagation()
        void goToDefinition()
        return
      }

      if (event.keyCode === monaco.KeyCode.F2) {
        event.preventDefault()
        event.stopPropagation()
        void openRename()
      }
    }))

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void useEditorStore.getState().saveTab(filePath)
    })

    editorDisposablesRef.current.push(editor.addAction({
      id: 'vs-monitor.go-to-definition',
      label: 'Go to Definition',
      contextMenuGroupId: 'navigation',
      contextMenuOrder: 0.8,
      run: async () => { await goToDefinition() },
    }))

    editorDisposablesRef.current.push(editor.addAction({
      id: 'vs-monitor.find-references',
      label: 'Find References',
      contextMenuGroupId: 'navigation',
      contextMenuOrder: 0.81,
      run: async () => { await showReferences() },
    }))

    editorDisposablesRef.current.push(editor.addAction({
      id: 'vs-monitor.rename-symbol',
      label: 'Rename Symbol',
      contextMenuGroupId: '1_modification',
      contextMenuOrder: 0.9,
      run: async () => { await openRename() },
    }))
  }

  return (
    <>
      <Editor
        path={path}
        defaultLanguage={modelLanguage}
        value={value}
        beforeMount={() => {
          ensureMonacoLoader()
          applyTheme(theme)
        }}
        onMount={handleEditorMount}
        onChange={handleEditorChange}
        options={{
          automaticLayout: true,
          minimap: { enabled: false },
          fontSize,
          fontFamily,
          wordWrap: wordWrap ? 'on' : 'off',
          lineNumbers: showLineNumbers ? 'on' : 'off',
          tabSize,
          smoothScrolling: true,
          cursorBlinking: 'smooth',
          padding: { top: 12 },
          renderLineHighlight: 'gutter',
          scrollBeyondLastLine: false,
        }}
      />
      {resultsDialog && createPortal(
        <div className="modal-overlay" onClick={() => setResultsDialog(null)}>
          <div className="modal symbol-modal" onClick={(event) => event.stopPropagation()}>
            <div className="symbol-modal-head">
              <div>
                <h3>{resultsDialog.title}</h3>
                <p>{resultsDialog.subtitle}</p>
              </div>
              <span className="symbol-modal-count">{resultsDialog.items.length} result{resultsDialog.items.length === 1 ? '' : 's'}</span>
            </div>
            <div className="symbol-results-list">
              {resultsDialog.items.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  className="symbol-result-item"
                  onClick={() => { void navigateToSymbolLocation(item) }}
                >
                  <div className="symbol-result-head">
                    <strong>{item.filePath.replace(/\\/g, '/').split('/').pop() || item.filePath}</strong>
                    <div className="symbol-result-badges">
                      {item.isDefinition && <span className="symbol-result-badge">Definition</span>}
                      {item.isWriteAccess && <span className="symbol-result-badge write">Write</span>}
                    </div>
                  </div>
                  <div className="symbol-result-path">{item.filePath}</div>
                  <div className="symbol-result-preview">{item.preview}</div>
                  <div className="symbol-result-meta">Ln {item.line}, Col {item.column}</div>
                </button>
              ))}
            </div>
            <div className="modal-actions">
              <button type="button" className="btn-cancel" onClick={() => setResultsDialog(null)}>
                Close
              </button>
            </div>
          </div>
        </div>,
        document.body
      )}
      {renameDialog && createPortal(
        <div className="modal-overlay" onClick={() => !renameDialog.pending && setRenameDialog(null)}>
          <div className="modal symbol-modal" onClick={(event) => event.stopPropagation()}>
            <div className="symbol-modal-head">
              <div>
                <h3>Rename Symbol</h3>
                <p>Apply a TypeScript rename across the loaded workspace models and save the touched files.</p>
              </div>
            </div>
            <div className="symbol-rename-copy">
              <span>Current name</span>
              <strong>{renameDialog.symbolName}</strong>
            </div>
            <input
              ref={renameInputRef}
              value={renameDialog.nextName}
              onChange={(event) => {
                const nextValue = event.target.value
                setRenameDialog((current) => current ? { ...current, nextName: nextValue } : current)
              }}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  void applyRename()
                }
                if (event.key === 'Escape' && !renameDialog.pending) {
                  event.preventDefault()
                  setRenameDialog(null)
                }
              }}
              disabled={renameDialog.pending}
            />
            <div className="symbol-rename-hint">Shortcut: `F2` opens rename. `Shift+F12` shows references before you commit.</div>
            <div className="modal-actions">
              <button
                type="button"
                className="btn-cancel"
                onClick={() => setRenameDialog(null)}
                disabled={renameDialog.pending}
              >
                Cancel
              </button>
              <button
                type="button"
                className="btn-confirm"
                onClick={() => { void applyRename() }}
                disabled={renameDialog.pending}
              >
                {renameDialog.pending ? 'Renaming...' : 'Rename'}
              </button>
            </div>
          </div>
        </div>,
        document.body
      )}
    </>
  )
}
