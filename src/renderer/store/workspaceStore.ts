import { create } from 'zustand'
import type { FileEntry } from '@shared/types/file'
import type { GitStatus } from '@shared/types/ipc'

const LAST_OPENED_FOLDER_KEY = 'lastOpenedFolder'
const RECENT_WORKSPACES_KEY = 'recentWorkspaces'
const MAX_RECENT_WORKSPACES = 8

function readRecentWorkspaces() {
  if (typeof window === 'undefined') return [] as string[]

  try {
    const raw = window.localStorage.getItem(RECENT_WORKSPACES_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((value): value is string => typeof value === 'string') : []
  } catch {
    return []
  }
}

function persistRecentWorkspaces(items: string[]) {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(RECENT_WORKSPACES_KEY, JSON.stringify(items))
}

function touchRecentWorkspace(items: string[], nextPath: string) {
  return [nextPath, ...items.filter((item) => item !== nextPath)].slice(0, MAX_RECENT_WORKSPACES)
}

interface WorkspaceState {
  rootPath: string | null
  recentWorkspaces: string[]
  revision: number
  tree: FileEntry[]
  isLoading: boolean
  openFolder: () => Promise<void>
  setRootPath: (path: string) => Promise<void>
  refreshTree: () => Promise<void>
  refreshGitStatus: () => Promise<void>
  gitStatus: GitStatus
  init: () => Promise<void>
}

export const useWorkspaceStore = create<WorkspaceState>((set, get) => ({
  rootPath: null,
  recentWorkspaces: readRecentWorkspaces(),
  revision: 0,
  tree: [],
  gitStatus: {},
  isLoading: false,

  openFolder: async () => {
    const path = await window.electronAPI.openFolder()
    if (path) {
      await get().setRootPath(path)
    }
  },

  setRootPath: async (rootPath: string) => {
    set({ isLoading: true })

    try {
      const resolvedRootPath = await window.electronAPI.setWorkspaceRoot(rootPath)
      if (!resolvedRootPath) {
        set({ isLoading: false })
        return
      }

      const tree = await window.electronAPI.readDir(resolvedRootPath)
      const recentWorkspaces = touchRecentWorkspace(get().recentWorkspaces, resolvedRootPath)
      set((state) => ({
        rootPath: resolvedRootPath,
        tree,
        isLoading: false,
        recentWorkspaces,
        revision: state.revision + 1,
      }))
      localStorage.setItem(LAST_OPENED_FOLDER_KEY, resolvedRootPath)
      persistRecentWorkspaces(recentWorkspaces)
      window.electronAPI.setTerminalCwd('default', resolvedRootPath)
      await get().refreshGitStatus()
    } catch (error) {
      console.error('Failed to set workspace root:', error)
      set({ isLoading: false })
    }
  },

  refreshTree: async () => {
    const { rootPath } = get()
    if (!rootPath) return
    set({ isLoading: true })
    const tree = await window.electronAPI.readDir(rootPath)
    set((state) => ({ tree, isLoading: false, revision: state.revision + 1 }))
    await get().refreshGitStatus()
  },

  refreshGitStatus: async () => {
    const { rootPath } = get()
    if (!rootPath) return
    const gitStatus = await window.electronAPI.getGitStatus(rootPath)
    set({ gitStatus })
  },

  init: async () => {
    const recentWorkspaces = readRecentWorkspaces()
    set({ recentWorkspaces })

    const lastFolder = localStorage.getItem(LAST_OPENED_FOLDER_KEY)
    if (lastFolder) {
      try {
        await get().setRootPath(lastFolder)
      } catch {
        localStorage.removeItem(LAST_OPENED_FOLDER_KEY)
      }
    }
  },
}))
