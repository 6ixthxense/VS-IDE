import { create } from 'zustand'
import type { Tab } from '@shared/types/file'

function getLanguage(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    js: 'javascript', ts: 'typescript', jsx: 'javascript', tsx: 'typescript',
    py: 'python', json: 'json', html: 'html', css: 'css', md: 'markdown',
    yml: 'yaml', yaml: 'yaml', sh: 'shell', txt: 'plaintext',
  }
  return map[ext] ?? 'plaintext'
}

interface EditorGroup {
  id: string
  tabIds: string[]
  activeTabId: string | null
}

interface EditorState {
  tabs: Tab[]
  groups: EditorGroup[]
  activeGroupId: string
  openFile: (filePath: string, groupId?: string) => Promise<void>
  closeTab: (tabId: string, groupId?: string) => void
  setContent: (tabId: string, content: string) => void
  saveTab: (tabId: string) => Promise<boolean>
  getActiveTab: (groupId?: string) => Tab | undefined
  setActiveGroupId: (id: string) => void
  splitGroup: (sourceGroupId: string, direction: 'horizontal' | 'vertical') => void
}

export const useEditorStore = create<EditorState>((set, get) => ({
  tabs: [],
  groups: [{ id: 'main', tabIds: [], activeTabId: null }],
  activeGroupId: 'main',

  openFile: async (filePath: string, groupId?: string) => {
    const targetGroup = groupId || get().activeGroupId
    const existingTab = get().tabs.find(t => t.path === filePath)
    
    if (existingTab) {
      set(s => ({
        activeGroupId: targetGroup,
        groups: s.groups.map(g => 
          g.id === targetGroup 
            ? { ...g, tabIds: g.tabIds.includes(filePath) ? g.tabIds : [...g.tabIds, filePath], activeTabId: filePath }
            : g
        )
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

    set(s => ({
      tabs: [...s.tabs, newTab],
      activeGroupId: targetGroup,
      groups: s.groups.map(g => 
        g.id === targetGroup 
          ? { ...g, tabIds: [...g.tabIds, filePath], activeTabId: filePath }
          : g
      )
    }))
  },

  closeTab: (tabId: string, groupId?: string) => {
    set(s => {
      const gId = groupId || s.activeGroupId
      const group = s.groups.find(g => g.id === gId)
      if (!group) return s

      const newTabIds = group.tabIds.filter(id => id !== tabId)
      let newActiveId = group.activeTabId
      if (group.activeTabId === tabId) {
        const idx = group.tabIds.indexOf(tabId)
        newActiveId = newTabIds[Math.max(0, idx - 1)] ?? null
      }

      // If no other group has this tab, remove it from global tabs list
      const isTabUsedElsewhere = s.groups.some(g => g.id !== gId && g.tabIds.includes(tabId))
      const newTabs = isTabUsedElsewhere ? s.tabs : s.tabs.filter(t => t.id !== tabId)

      return {
        tabs: newTabs,
        groups: s.groups.map(g => 
          g.id === gId ? { ...g, tabIds: newTabIds, activeTabId: newActiveId } : g
        )
      }
    })
  },

  setContent: (tabId: string, content: string) => {
    set((s) => {
      const tab = s.tabs.find((t) => t.id === tabId)
      if (!tab) return s
      const normNew = content.replace(/\r\n/g, '\n')
      if (tab.content === normNew) return s
      return {
        tabs: s.tabs.map((t) => t.id === tabId ? { ...t, content: normNew } : t),
      }
    })
  },

  saveTab: async (tabId: string) => {
    const tab = get().tabs.find(t => t.id === tabId)
    if (!tab) return false
    const ok = await window.electronAPI.writeFile(tab.path, tab.content)
    if (ok) {
      set(s => ({
        tabs: s.tabs.map(t => t.id === tabId ? { ...t, savedContent: t.content } : t),
      }))
    }
    return ok
  },

  getActiveTab: (groupId?: string) => {
    const gId = groupId || get().activeGroupId
    const group = get().groups.find(g => g.id === gId)
    if (!group || !group.activeTabId) return undefined
    return get().tabs.find(t => t.id === group.activeTabId)
  },

  setActiveGroupId: (id: string) => set({ activeGroupId: id }),

  splitGroup: (sourceGroupId: string, direction: 'horizontal' | 'vertical') => {
    // Basic implementation: split main or whatever is there
    const id = `group-${Date.now()}`
    set(s => ({
      groups: [...s.groups, { id, tabIds: [], activeTabId: null }],
      activeGroupId: id
    }))
  }
}))
