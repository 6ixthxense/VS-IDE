import type { Tab } from '@shared/types/file'

const STORAGE_PREFIX = 'vs-monitor-ide'
const MAX_PERSISTED_TAB_SIZE = 200_000
const MAX_PERSISTED_TAB_BUDGET = 1_500_000

export type PersistedSidebarView = 'explorer' | 'search' | 'git'
export type PersistedEditorLayoutDirection = 'horizontal' | 'vertical'

export interface PersistedEditorGroup {
  id: string
  tabIds: string[]
  activeTabId: string | null
}

export interface PersistedEditorTab {
  id: string
  path: string
  name: string
  language: string
  content?: string
  savedContent?: string
}

export interface PersistedEditorSession {
  tabs: PersistedEditorTab[]
  groups: PersistedEditorGroup[]
  activeGroupId: string
  layoutDirection: PersistedEditorLayoutDirection
}

export interface PersistedWorkbenchUi {
  showTerminal: boolean
  showSysMonitor: boolean
  activeSidebarView: PersistedSidebarView
  showSidebar: boolean
}

export interface PersistedTerminalEntry {
  id: string
  cwd: string
}

export interface PersistedTerminalSession {
  terminals: PersistedTerminalEntry[]
  activeId: string
}

function readStorage<T>(key: string): T | null {
  if (typeof window === 'undefined') return null

  try {
    const raw = window.localStorage.getItem(key)
    return raw ? JSON.parse(raw) as T : null
  } catch {
    return null
  }
}

function writeStorage(key: string, value: unknown) {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(key, JSON.stringify(value))
}

function removeStorage(key: string) {
  if (typeof window === 'undefined') return
  window.localStorage.removeItem(key)
}

function normalizeWorkspaceKey(rootPath: string) {
  return encodeURIComponent(rootPath.replace(/\\/g, '/').toLowerCase())
}

function buildWorkspaceKey(rootPath: string, section: string) {
  return `${STORAGE_PREFIX}:workspace:${normalizeWorkspaceKey(rootPath)}:${section}`
}

function buildGlobalKey(section: string) {
  return `${STORAGE_PREFIX}:global:${section}`
}

export function serializeTabsForSession(tabs: Tab[]): PersistedEditorTab[] {
  let budget = 0

  return tabs.map((tab) => {
    const payload = tab.content.length + tab.savedContent.length
    const canPersistContent =
      payload <= MAX_PERSISTED_TAB_SIZE &&
      budget + payload <= MAX_PERSISTED_TAB_BUDGET

    if (canPersistContent) {
      budget += payload
    }

    return {
      id: tab.id,
      path: tab.path,
      name: tab.name,
      language: tab.language,
      content: canPersistContent ? tab.content : undefined,
      savedContent: canPersistContent ? tab.savedContent : undefined,
    }
  })
}

export function readEditorSession(rootPath: string) {
  return readStorage<PersistedEditorSession>(buildWorkspaceKey(rootPath, 'editor'))
}

export function writeEditorSession(rootPath: string, snapshot: PersistedEditorSession) {
  writeStorage(buildWorkspaceKey(rootPath, 'editor'), snapshot)
}

export function clearEditorSession(rootPath: string) {
  removeStorage(buildWorkspaceKey(rootPath, 'editor'))
}

export function readTerminalSession(rootPath: string) {
  return readStorage<PersistedTerminalSession>(buildWorkspaceKey(rootPath, 'terminal'))
}

export function writeTerminalSession(rootPath: string, snapshot: PersistedTerminalSession) {
  writeStorage(buildWorkspaceKey(rootPath, 'terminal'), snapshot)
}

export function clearTerminalSession(rootPath: string) {
  removeStorage(buildWorkspaceKey(rootPath, 'terminal'))
}

export function readWorkbenchUi() {
  return readStorage<PersistedWorkbenchUi>(buildGlobalKey('ui'))
}

export function writeWorkbenchUi(snapshot: PersistedWorkbenchUi) {
  writeStorage(buildGlobalKey('ui'), snapshot)
}
