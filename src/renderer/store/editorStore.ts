import { create } from 'zustand'
import type { Tab } from '@shared/types/file'
import { confirmAction, showErrorToast, showInfoToast, showWarningToast } from './feedbackStore'
import { readEditorSession, serializeTabsForSession, writeEditorSession } from '../utils/workbenchPersistence'
import { resolveRestoredTab, syncTabWithDisk } from '../utils/editorRecovery'

type EditorLayoutDirection = 'horizontal' | 'vertical'
const RECENT_FILES_KEY = 'recentFiles'
const MAX_RECENT_FILES = 24
const MAX_CLOSED_TABS = 12

function readRecentFiles() {
  if (typeof window === 'undefined') return [] as string[]

  try {
    const raw = window.localStorage.getItem(RECENT_FILES_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === 'string') : []
  } catch {
    return []
  }
}

function persistRecentFiles(items: string[]) {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(RECENT_FILES_KEY, JSON.stringify(items))
}

function touchRecentFile(items: string[], filePath: string) {
  return [filePath, ...items.filter((item) => item !== filePath)].slice(0, MAX_RECENT_FILES)
}

function rememberClosedTab(items: Tab[], tab: Tab) {
  return [tab, ...items.filter((item) => item.id !== tab.id)].slice(0, MAX_CLOSED_TABS)
}

function getLanguage(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    js: 'javascript', ts: 'typescript', jsx: 'javascript', tsx: 'typescript',
    py: 'python', json: 'json', html: 'html', css: 'css', md: 'markdown',
    yml: 'yaml', yaml: 'yaml', sql: 'sql', sh: 'shell', txt: 'plaintext',
  }
  return map[ext] ?? 'plaintext'
}

function createGroup(id: string, tabIds: string[] = [], activeTabId: string | null = null) {
  return { id, tabIds, activeTabId }
}

function getReferencedTabIds(groups: EditorGroup[]): Set<string> {
  return new Set(groups.flatMap((group) => group.tabIds))
}

function getNextActiveGroupId(groups: EditorGroup[], preferredGroupId: string | null): string {
  if (preferredGroupId && groups.some((group) => group.id === preferredGroupId)) {
    return preferredGroupId
  }

  return groups[0]?.id ?? 'main'
}

function normalizeGroups(
  groups: EditorGroup[],
  activeGroupId: string | null,
  layoutDirection: EditorLayoutDirection
) {
  if (groups.length === 0) {
    return {
      groups: [createGroup('main')],
      activeGroupId: 'main',
      layoutDirection: 'horizontal' as const,
    }
  }

  return {
    groups,
    activeGroupId: getNextActiveGroupId(groups, activeGroupId),
    layoutDirection: groups.length > 1 ? layoutDirection : 'horizontal' as const,
  }
}

function persistEditorSession(rootPath: string | null, state: Pick<EditorState, 'tabs' | 'groups' | 'activeGroupId' | 'layoutDirection'>) {
  if (!rootPath) return

  writeEditorSession(rootPath, {
    tabs: serializeTabsForSession(state.tabs),
    groups: state.groups.map((group) => ({
      id: group.id,
      tabIds: [...group.tabIds],
      activeTabId: group.activeTabId,
    })),
    activeGroupId: state.activeGroupId,
    layoutDirection: state.layoutDirection,
  })
}

function buildPersistedEditorState(state: EditorState, nextState: Partial<EditorState>) {
  const persistedState = {
    tabs: nextState.tabs ?? state.tabs,
    groups: nextState.groups ?? state.groups,
    activeGroupId: nextState.activeGroupId ?? state.activeGroupId,
    layoutDirection: nextState.layoutDirection ?? state.layoutDirection,
  }

  persistEditorSession(state.sessionWorkspaceRoot, persistedState)
  return nextState
}

function createRestoredGroupSet(groups: EditorGroup[], tabs: Tab[]) {
  const availableTabIds = new Set(tabs.map((tab) => tab.id))
  const nextGroups = groups
    .map((group) => {
      const tabIds = group.tabIds.filter((tabId) => availableTabIds.has(tabId))
      return {
        id: group.id,
        tabIds,
        activeTabId: group.activeTabId && tabIds.includes(group.activeTabId) ? group.activeTabId : tabIds[0] ?? null,
      }
    })
    .filter((group) => group.tabIds.length > 0)

  if (nextGroups.length > 0) {
    return nextGroups
  }

  return tabs.length > 0 ? [createGroup('main', tabs.map((tab) => tab.id), tabs[0].id)] : [createGroup('main')]
}

interface EditorGroup {
  id: string
  tabIds: string[]
  activeTabId: string | null
}

interface EditorState {
  sessionWorkspaceRoot: string | null
  tabs: Tab[]
  recentFiles: string[]
  closedTabs: Tab[]
  groups: EditorGroup[]
  activeGroupId: string
  layoutDirection: EditorLayoutDirection
  openFile: (filePath: string, groupId?: string) => Promise<void>
  closeTab: (tabId: string, groupId?: string) => void
  closeGroup: (groupId: string) => void
  setContent: (tabId: string, content: string) => void
  saveTab: (tabId: string) => Promise<boolean>
  reloadTabFromDisk: (tabId: string) => Promise<boolean>
  dismissTabRecovery: (tabId: string) => void
  syncOpenTabsWithDisk: () => Promise<void>
  getActiveTab: (groupId?: string) => Tab | undefined
  setActiveGroupId: (id: string) => void
  setActiveTab: (groupId: string, tabId: string) => void
  splitGroup: (sourceGroupId: string, direction: EditorLayoutDirection) => void
  reopenClosedTab: (groupId?: string) => boolean
  restoreSessionForWorkspace: (rootPath: string | null) => Promise<void>
}

export const useEditorStore = create<EditorState>((set, get) => ({
  sessionWorkspaceRoot: null,
  tabs: [],
  recentFiles: readRecentFiles(),
  closedTabs: [],
  groups: [createGroup('main')],
  activeGroupId: 'main',
  layoutDirection: 'horizontal',

  openFile: async (filePath: string, groupId?: string) => {
    const targetGroup = groupId || get().activeGroupId
    const existingTab = get().tabs.find((tab) => tab.path === filePath)
    const nextRecentFiles = touchRecentFile(get().recentFiles, filePath)
    persistRecentFiles(nextRecentFiles)

    if (existingTab) {
      set((state) => {
        const nextState = {
          recentFiles: nextRecentFiles,
          closedTabs: state.closedTabs.filter((tab) => tab.id !== filePath),
          activeGroupId: targetGroup,
          groups: state.groups.map((group) =>
            group.id === targetGroup
              ? {
                  ...group,
                  tabIds: group.tabIds.includes(filePath) ? group.tabIds : [...group.tabIds, filePath],
                  activeTabId: filePath,
                }
              : group
          ),
        }

        return buildPersistedEditorState(state, nextState)
      })
      return
    }

    const exists = await window.electronAPI.pathExists(filePath)
    if (!exists) {
      showErrorToast('The file could not be found anymore.', 'Open file failed', filePath)
      return
    }

    const content = await window.electronAPI.readFile(filePath)
    const name = filePath.replace(/\\/g, '/').split('/').pop() ?? filePath
    const safeContent = content ?? ''

    const newTab: Tab = {
      id: filePath,
      path: filePath,
      name,
      content: safeContent,
      savedContent: safeContent,
      language: getLanguage(filePath),
      recoveryState: undefined,
      recoveryMessage: undefined,
    }

    set((state) => {
      const nextState = {
        tabs: [...state.tabs, newTab],
        recentFiles: nextRecentFiles,
        closedTabs: state.closedTabs.filter((tab) => tab.id !== filePath),
        activeGroupId: targetGroup,
        groups: state.groups.map((group) =>
          group.id === targetGroup
            ? { ...group, tabIds: [...group.tabIds, filePath], activeTabId: filePath }
            : group
        ),
      }

      return buildPersistedEditorState(state, nextState)
    })
  },

  closeTab: (tabId: string, groupId?: string) => {
    set((state) => {
      const targetGroupId = groupId || state.activeGroupId
      const targetGroupIndex = state.groups.findIndex((group) => group.id === targetGroupId)
      if (targetGroupIndex === -1) return state

      const closingTab = state.tabs.find((tab) => tab.id === tabId)
      const targetGroup = state.groups[targetGroupIndex]
      const newTabIds = targetGroup.tabIds.filter((id) => id !== tabId)
      let newActiveTabId = targetGroup.activeTabId

      if (targetGroup.activeTabId === tabId) {
        const closedTabIndex = targetGroup.tabIds.indexOf(tabId)
        newActiveTabId = newTabIds[Math.max(0, closedTabIndex - 1)] ?? null
      }

      let nextGroups = state.groups.map((group) =>
        group.id === targetGroupId
          ? { ...group, tabIds: newTabIds, activeTabId: newActiveTabId }
          : group
      )

      if (newTabIds.length === 0 && nextGroups.length > 1) {
        nextGroups = nextGroups.filter((group) => group.id !== targetGroupId)
      }

      const referencedTabIds = getReferencedTabIds(nextGroups)
      const nextTabs = state.tabs.filter((tab) => referencedTabIds.has(tab.id))
      const nextClosedTabs = closingTab && !referencedTabIds.has(tabId)
        ? rememberClosedTab(state.closedTabs, closingTab)
        : state.closedTabs
      const preferredGroupId =
        nextGroups.some((group) => group.id === state.activeGroupId)
          ? state.activeGroupId
          : nextGroups[Math.max(0, targetGroupIndex - 1)]?.id ?? nextGroups[0]?.id ?? null

      const nextState = {
        tabs: nextTabs,
        closedTabs: nextClosedTabs,
        ...normalizeGroups(nextGroups, preferredGroupId, state.layoutDirection),
      }

      return buildPersistedEditorState(state, nextState)
    })
  },

  closeGroup: (groupId: string) => {
    set((state) => {
      if (state.groups.length === 1) return state

      const groupIndex = state.groups.findIndex((group) => group.id === groupId)
      if (groupIndex === -1) return state

      const nextGroups = state.groups.filter((group) => group.id !== groupId)
      const referencedTabIds = getReferencedTabIds(nextGroups)
      const nextTabs = state.tabs.filter((tab) => referencedTabIds.has(tab.id))
      const nextClosedTabs = state.tabs
        .filter((tab) => !referencedTabIds.has(tab.id))
        .reduce((acc, tab) => rememberClosedTab(acc, tab), state.closedTabs)
      const preferredGroupId =
        state.activeGroupId === groupId
          ? nextGroups[Math.max(0, groupIndex - 1)]?.id ?? nextGroups[0]?.id ?? null
          : state.activeGroupId

      const nextState = {
        tabs: nextTabs,
        closedTabs: nextClosedTabs,
        ...normalizeGroups(nextGroups, preferredGroupId, state.layoutDirection),
      }

      return buildPersistedEditorState(state, nextState)
    })
  },

  setContent: (tabId: string, content: string) => {
    set((state) => {
      const tab = state.tabs.find((item) => item.id === tabId)
      if (!tab) return state
      const normalizedContent = content.replace(/\r\n/g, '\n')
      if (tab.content === normalizedContent) return state

      const nextState = {
        tabs: state.tabs.map((item) =>
          item.id === tabId ? { ...item, content: normalizedContent } : item
        ),
      }

      return buildPersistedEditorState(state, nextState)
    })
  },

  saveTab: async (tabId: string) => {
    const tab = get().tabs.find((item) => item.id === tabId)
    if (!tab) return false

    const exists = await window.electronAPI.pathExists(tab.path)
    if (!exists) {
      set((state) => buildPersistedEditorState(state, {
        tabs: state.tabs.map((item) =>
          item.id === tabId
            ? { ...item, recoveryState: 'conflict', recoveryMessage: 'This file is no longer available on disk.' }
            : item
        ),
      }))
      showErrorToast('The file no longer exists on disk.', 'Save failed', tab.path)
      return false
    }

    const diskContent = await window.electronAPI.readFile(tab.path)
    if (diskContent === tab.content) {
      set((state) => {
        const nextState = {
          tabs: state.tabs.map((item) =>
            item.id === tabId
              ? { ...item, savedContent: diskContent, recoveryState: undefined, recoveryMessage: undefined }
              : item
          ),
        }

        return buildPersistedEditorState(state, nextState)
      })
      return true
    }

    if (diskContent !== tab.savedContent) {
      const shouldOverwrite = await confirmAction({
        title: 'Overwrite newer disk changes?',
        message: `${tab.name} changed on disk after you opened it. Saving now will replace the newer disk version with your local editor content.`,
        confirmLabel: 'Overwrite Disk',
        cancelLabel: 'Keep Reviewing',
        tone: 'warning',
      })

      if (!shouldOverwrite) {
        set((state) => buildPersistedEditorState(state, {
          tabs: state.tabs.map((item) =>
            item.id === tabId
              ? {
                  ...item,
                  recoveryState: 'conflict',
                  recoveryMessage: 'Save paused because the file changed on disk. Review the conflict before overwriting it.',
                }
              : item
          ),
        }))
        showWarningToast('The newer disk version was kept. Review the conflict banner before saving again.', 'Save paused')
        return false
      }
    }

    const result = await window.electronAPI.writeFile(tab.path, tab.content)
    if (result.success) {
      set((state) => {
        const nextState = {
          tabs: state.tabs.map((item) =>
            item.id === tabId
              ? { ...item, savedContent: item.content, recoveryState: undefined, recoveryMessage: undefined }
              : item
          ),
        }

        return buildPersistedEditorState(state, nextState)
      })
    } else {
      showErrorToast(result.error || 'Unable to save file.', 'Save failed', result.details || tab.path)
    }

    return result.success
  },

  reloadTabFromDisk: async (tabId: string) => {
    const tab = get().tabs.find((item) => item.id === tabId)
    if (!tab) return false

    const exists = await window.electronAPI.pathExists(tab.path)
    if (!exists) {
      showErrorToast('The file could not be found on disk anymore.', 'Reload failed', tab.path)
      return false
    }

    const diskContent = await window.electronAPI.readFile(tab.path)
    set((state) => {
      const nextState = {
        tabs: state.tabs.map((item) =>
          item.id === tabId
            ? {
                ...item,
                content: diskContent,
                savedContent: diskContent,
                recoveryState: undefined,
                recoveryMessage: undefined,
              }
            : item
        ),
      }

      return buildPersistedEditorState(state, nextState)
    })

    showInfoToast(`${tab.name} reloaded from disk.`, 'Disk version restored')
    return true
  },

  dismissTabRecovery: (tabId: string) => {
    set((state) => buildPersistedEditorState(state, {
      tabs: state.tabs.map((item) =>
        item.id === tabId
          ? { ...item, recoveryState: undefined, recoveryMessage: undefined }
          : item
      ),
    }))
  },

  syncOpenTabsWithDisk: async () => {
    const state = get()
    if (state.tabs.length === 0) return

    const nextTabs: Tab[] = []
    const reloadedTabs: string[] = []
    const conflictTabs: string[] = []
    const missingTabs: string[] = []
    let hasChanges = false

    for (const tab of state.tabs) {
      const exists = await window.electronAPI.pathExists(tab.path)
      const diskContent = exists ? await window.electronAPI.readFile(tab.path) : ''
      const resolution = syncTabWithDisk(tab, exists, diskContent)

      if (resolution.effect !== 'unchanged') {
        hasChanges = true
      }
      if (resolution.effect === 'reloaded') {
        reloadedTabs.push(tab.name)
      } else if (resolution.effect === 'conflict') {
        conflictTabs.push(tab.name)
      } else if (resolution.effect === 'missing') {
        missingTabs.push(tab.name)
      }

      nextTabs.push(resolution.tab)
    }

    if (!hasChanges) return

    set((current) => buildPersistedEditorState(current, { tabs: nextTabs }))

    if (reloadedTabs.length > 0) {
      showInfoToast(
        reloadedTabs.length === 1
          ? `${reloadedTabs[0]} was refreshed from disk.`
          : `${reloadedTabs.length} open files were refreshed from disk.`,
        'External changes loaded'
      )
    }

    if (conflictTabs.length > 0) {
      showWarningToast(
        conflictTabs.length === 1
          ? `${conflictTabs[0]} has both local edits and newer disk changes.`
          : `${conflictTabs.length} open files now have local-vs-disk conflicts.`,
        'Review file conflicts'
      )
    }

    if (missingTabs.length > 0) {
      showWarningToast(
        missingTabs.length === 1
          ? `${missingTabs[0]} was removed from disk.`
          : `${missingTabs.length} open files are missing from disk.`,
        'Files missing on disk'
      )
    }
  },

  getActiveTab: (groupId?: string) => {
    const targetGroupId = groupId || get().activeGroupId
    const group = get().groups.find((item) => item.id === targetGroupId)
    if (!group || !group.activeTabId) return undefined
    return get().tabs.find((tab) => tab.id === group.activeTabId)
  },

  setActiveGroupId: (id: string) => set((state) => buildPersistedEditorState(state, { activeGroupId: id })),

  setActiveTab: (groupId: string, tabId: string) => {
    set((state) => {
      const nextState = {
        activeGroupId: groupId,
        groups: state.groups.map((group) =>
          group.id === groupId ? { ...group, activeTabId: tabId } : group
        ),
      }

      return buildPersistedEditorState(state, nextState)
    })
  },

  splitGroup: (sourceGroupId: string, direction: EditorLayoutDirection) => {
    set((state) => {
      const sourceGroupIndex = state.groups.findIndex((group) => group.id === sourceGroupId)
      if (sourceGroupIndex === -1) return state

      const sourceGroup = state.groups[sourceGroupIndex]
      if (!sourceGroup.activeTabId) return state

      const newGroupId = `group-${Date.now()}`
      const newGroup = createGroup(newGroupId, [sourceGroup.activeTabId], sourceGroup.activeTabId)
      const nextGroups = [...state.groups]
      nextGroups.splice(sourceGroupIndex + 1, 0, newGroup)

      const nextState = {
        groups: nextGroups,
        activeGroupId: newGroupId,
        layoutDirection: direction,
      }

      return buildPersistedEditorState(state, nextState)
    })
  },

  reopenClosedTab: (groupId?: string) => {
    const targetGroupId = groupId || get().activeGroupId
    let reopened = false

    set((state) => {
      const targetGroup = state.groups.find((group) => group.id === targetGroupId)
      const tabToRestore = state.closedTabs[0]

      if (!targetGroup || !tabToRestore) {
        return state
      }

      const existingTab = state.tabs.find((tab) => tab.id === tabToRestore.id)
      const nextRecentFiles = touchRecentFile(state.recentFiles, tabToRestore.path)
      persistRecentFiles(nextRecentFiles)
      reopened = true

      const nextState = {
        tabs: existingTab ? state.tabs : [...state.tabs, tabToRestore],
        recentFiles: nextRecentFiles,
        closedTabs: state.closedTabs.filter((tab) => tab.id !== tabToRestore.id),
        activeGroupId: targetGroupId,
        groups: state.groups.map((group) =>
          group.id === targetGroupId
            ? {
                ...group,
                tabIds: group.tabIds.includes(tabToRestore.id) ? group.tabIds : [...group.tabIds, tabToRestore.id],
                activeTabId: tabToRestore.id,
              }
            : group
        ),
      }

      return buildPersistedEditorState(state, nextState)
    })

    return reopened
  },

  restoreSessionForWorkspace: async (rootPath) => {
    if (!rootPath) {
      set({
        sessionWorkspaceRoot: null,
        tabs: [],
        closedTabs: [],
        groups: [createGroup('main')],
        activeGroupId: 'main',
        layoutDirection: 'horizontal',
      })
      return
    }

    const snapshot = readEditorSession(rootPath)
    if (!snapshot) {
      set({
        sessionWorkspaceRoot: rootPath,
        tabs: [],
        closedTabs: [],
        groups: [createGroup('main')],
        activeGroupId: 'main',
        layoutDirection: 'horizontal',
      })
      persistEditorSession(rootPath, {
        tabs: [],
        groups: [createGroup('main')],
        activeGroupId: 'main',
        layoutDirection: 'horizontal',
      })
      return
    }

    const restoredTabs: Tab[] = []
    let recoveredCount = 0
    let conflictCount = 0

    for (const tab of snapshot.tabs) {
      const exists = await window.electronAPI.pathExists(tab.path)
      if (!exists) continue

      const diskContent = await window.electronAPI.readFile(tab.path)
      const resolution = resolveRestoredTab({
        ...tab,
        language: tab.language || getLanguage(tab.path),
      }, diskContent)

      if (resolution.issue === 'recovered') {
        recoveredCount += 1
      } else if (resolution.issue === 'conflict') {
        conflictCount += 1
      }

      restoredTabs.push(resolution.tab)
    }

    const groups = createRestoredGroupSet(snapshot.groups, restoredTabs)
    const normalized = normalizeGroups(
      groups,
      snapshot.activeGroupId,
      snapshot.layoutDirection === 'vertical' ? 'vertical' : 'horizontal'
    )

    set({
      sessionWorkspaceRoot: rootPath,
      tabs: restoredTabs,
      closedTabs: [],
      groups: normalized.groups,
      activeGroupId: normalized.activeGroupId,
      layoutDirection: normalized.layoutDirection,
    })

    persistEditorSession(rootPath, {
      tabs: restoredTabs,
      groups: normalized.groups,
      activeGroupId: normalized.activeGroupId,
      layoutDirection: normalized.layoutDirection,
    })

    if (recoveredCount > 0) {
      showInfoToast(
        recoveredCount === 1
          ? 'Recovered unsaved changes for 1 file from your last session.'
          : `Recovered unsaved changes for ${recoveredCount} files from your last session.`,
        'Recovered draft changes'
      )
    }

    if (conflictCount > 0) {
      showWarningToast(
        conflictCount === 1
          ? '1 restored file also changed on disk. Review the conflict banner before saving.'
          : `${conflictCount} restored files also changed on disk. Review the conflict banners before saving.`,
        'Restored conflicts need review'
      )
    }
  },
}))
