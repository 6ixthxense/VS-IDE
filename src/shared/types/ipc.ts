import type { FileEntry } from './file'

export interface SysStats {
  cpu: string
  ram: string
  ramText: string
  gpuName: string
  gpu?: {
    load: number
    temp: number
    memTotal: number
    memUsed: number
  } | null
  disk: { use: string; size: string; used: string } | null
  processes: { name: string; cpu: string; mem: string; pid: number }[]
}

export interface RunCodePayload {
  code: string
  language: 'javascript' | 'python'
}

export type GitFileStatus = 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'staged'

export interface GitStatus {
  [filePath: string]: GitFileStatus
}

export interface ElectronAPI {
  openFolder: () => Promise<string | null>
  readDir: (path: string) => Promise<FileEntry[]>
  readFile: (path: string) => Promise<string>
  writeFile: (path: string, content: string) => Promise<boolean>
  createFile: (dirPath: string, fileName: string) => Promise<{ success: boolean; path?: string; error?: string }>
  createDirectory: (dirPath: string, folderName: string) => Promise<{ success: boolean; path?: string; error?: string }>
  deletePath: (targetPath: string) => Promise<{ success: boolean; error?: string }>
  renamePath: (oldPath: string, newPath: string) => Promise<{ success: boolean; error?: string }>
  movePath: (oldPath: string, newPath: string) => Promise<{ success: boolean; error?: string }>
  revealInExplorer: (path: string) => void
  isWindows: boolean
  pathExists: (path: string) => Promise<boolean>
  getWorkspace: () => Promise<string>
  runCode: (payload: RunCodePayload) => void
  onSysStats: (cb: (stats: SysStats) => void) => () => void
  sendTerminalInput: (id: string, data: string) => void
  resizeTerminal: (id: string, cols: number, rows: number) => void
  setTerminalCwd: (id: string, path: string) => void
  createTerminal: (id: string, rootPath: string) => void
  closeTerminal: (id: string) => void
  onTerminalOut: (cb: (data: { id: string; data: string }) => void) => () => void
  getGitStatus: (rootPath: string) => Promise<GitStatus>
  gitAdd: (rootPath: string, filePaths: string[]) => Promise<boolean>
  gitCommit: (rootPath: string, message: string) => Promise<boolean>
  gitPush: (rootPath: string) => Promise<boolean>
  gitPull: (rootPath: string) => Promise<boolean>
  searchFiles: (query: string, rootPath: string) => Promise<{ path: string; name: string; line: number; match: string }[]>
  replaceInFiles: (query: string, replacement: string, rootPath: string, targetPath?: string) => Promise<{ success: boolean; count: number; error?: string }>
  getAllFiles: (rootPath: string) => Promise<string[]>
}

declare global {
  interface Window {
    electronAPI: ElectronAPI
  }
}
