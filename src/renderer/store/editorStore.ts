import { create } from 'zustand'
import type { Tab } from '@shared/types/file'

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
    yml: 'yaml', yaml: 'yaml', sh: 'shell', txt: 'plaintext',
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

interface EditorGroup {
  id: string
  tabIds: string[]
  activeTabId: string | null
}

interface EditorState {
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
  getActiveTab: (groupId?: string) => Tab | undefined
  setActiveGroupId: (id: string) => void
  setActiveTab: (groupId: string, tabId: string) => void
  splitGroup: (sourceGroupId: string, direction: EditorLayoutDirection) => void
  reopenClosedTab: (groupId?: string) => boolean
}

export const useEditorStore = create<EditorState>((set, get) => ({
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
      set((state) => ({
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
      }))
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
    }

    set((state) => ({
      tabs: [...state.tabs, newTab],
      recentFiles: nextRecentFiles,
      closedTabs: state.closedTabs.filter((tab) => tab.id !== filePath),
      activeGroupId: targetGroup,
      groups: state.groups.map((group) =>
        group.id === targetGroup
          ? { ...group, tabIds: [...group.tabIds, filePath], activeTabId: filePath }
          : group
      ),
    }))
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

      return {
        tabs: nextTabs,
        closedTabs: nextClosedTabs,
        ...normalizeGroups(nextGroups, preferredGroupId, state.layoutDirection),
      }
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

      return {
        tabs: nextTabs,
        closedTabs: nextClosedTabs,
        ...normalizeGroups(nextGroups, preferredGroupId, state.layoutDirection),
      }
    })
  },

  setContent: (tabId: string, content: string) => {
    set((state) => {
      const tab = state.tabs.find((item) => item.id === tabId)
      if (!tab) return state
      const normalizedContent = content.replace(/\r\n/g, '\n')
      if (tab.content === normalizedContent) return state

      return {
        tabs: state.tabs.map((item) =>
          item.id === tabId ? { ...item, content: normalizedContent } : item
        ),
      }
    })
  },

  saveTab: async (tabId: string) => {
    const tab = get().tabs.find((item) => item.id === tabId)
    if (!tab) return false

    const ok = await window.electronAPI.writeFile(tab.path, tab.content)
    if (ok) {
      set((state) => ({
        tabs: state.tabs.map((item) =>
          item.id === tabId ? { ...item, savedContent: item.content } : item
        ),
      }))
    }

    return ok
  },

  getActiveTab: (groupId?: string) => {
    const targetGroupId = groupId || get().activeGroupId
    const group = get().groups.find((item) => item.id === targetGroupId)
    if (!group || !group.activeTabId) return undefined
    return get().tabs.find((tab) => tab.id === group.activeTabId)
  },

  setActiveGroupId: (id: string) => set({ activeGroupId: id }),

  setActiveTab: (groupId: string, tabId: string) => {
    set((state) => ({
      activeGroupId: groupId,
      groups: state.groups.map((group) =>
        group.id === groupId ? { ...group, activeTabId: tabId } : group
      ),
    }))
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

      return {
        groups: nextGroups,
        activeGroupId: newGroupId,
        layoutDirection: direction,
      }
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

      return {
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
    })

    return reopened
  },
}))
