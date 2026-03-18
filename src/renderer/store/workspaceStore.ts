import { create } from 'zustand'
import type { FileEntry } from '@shared/types/file'
import type { GitStatus } from '@shared/types/ipc'

interface WorkspaceState {
  rootPath: string | null
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
    set({ rootPath, isLoading: true })
    const tree = await window.electronAPI.readDir(rootPath)
    set({ tree, isLoading: false })
    localStorage.setItem('lastOpenedFolder', rootPath)
    // Also sync to terminal
    window.electronAPI.setTerminalCwd('default', rootPath)
    // Fetch Git status
    get().refreshGitStatus()
  },

  refreshTree: async () => {
    const { rootPath } = get()
    if (!rootPath) return
    set({ isLoading: true })
    const tree = await window.electronAPI.readDir(rootPath)
    set({ tree, isLoading: false })
    get().refreshGitStatus()
  },

  refreshGitStatus: async () => {
    const { rootPath } = get()
    if (!rootPath) return
    const gitStatus = await window.electronAPI.getGitStatus(rootPath)
    set({ gitStatus })
  },

  init: async () => {
    const lastFolder = localStorage.getItem('lastOpenedFolder')
    if (lastFolder) {
      const exists = await window.electronAPI.pathExists(lastFolder)
      if (exists) {
        await get().setRootPath(lastFolder)
      } else {
        localStorage.removeItem('lastOpenedFolder')
      }
    }
  },
}))
