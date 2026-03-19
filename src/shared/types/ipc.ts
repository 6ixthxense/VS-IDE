import type { FileEntry } from './file'

export interface DiskStat {
  id: string
  name: string
  filesystem: string
  mount: string
  use: string
  size: string
  used: string
}

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
  disks: DiskStat[]
  processes: { name: string; cpu: string; mem: string; pid: number }[]
}

export interface TerminalOutputEvent {
  id: string
  data: string
}

export interface WorkspaceChangedEvent {
  timestamp: number
}

export interface SearchOptions {
  caseSensitive: boolean
  wholeWord: boolean
  useRegex: boolean
  includePattern?: string
  excludePattern?: string
}

export interface SearchResult {
  path: string
  name: string
  line: number
  match: string
}

export interface RunCodePayload {
  terminalId: string
  code: string
  language: 'javascript' | 'python'
}

export type GitFileStatus = 'modified' | 'added' | 'deleted' | 'renamed' | 'untracked' | 'staged'

export interface GitStatus {
  [filePath: string]: GitFileStatus
}

export interface GitDiffRequest {
  filePath: string
  staged?: boolean
  status?: GitFileStatus | string
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
  setWorkspaceRoot: (path: string | null) => Promise<string | null>
  runCode: (payload: RunCodePayload) => void
  onSysStats: (cb: (stats: SysStats) => void) => () => void
  onWorkspaceChanged: (cb: (event: WorkspaceChangedEvent) => void) => () => void
  sendTerminalInput: (id: string, data: string) => void
  resizeTerminal: (id: string, cols: number, rows: number) => void
  setTerminalCwd: (id: string, path: string) => void
  createTerminal: (id: string, rootPath: string) => void
  closeTerminal: (id: string) => void
  onTerminalOut: (cb: (data: TerminalOutputEvent) => void) => () => void
  getGitStatus: (rootPath: string) => Promise<GitStatus>
  gitAdd: (rootPath: string, filePaths: string[]) => Promise<boolean>
  gitUnstage: (rootPath: string, filePaths: string[]) => Promise<boolean>
  gitDiscard: (rootPath: string, request: GitDiffRequest) => Promise<boolean>
  gitCommit: (rootPath: string, message: string) => Promise<boolean>
  gitPush: (rootPath: string) => Promise<boolean>
  gitPull: (rootPath: string) => Promise<boolean>
  getGitDiff: (rootPath: string, request: GitDiffRequest) => Promise<string>
  searchFiles: (query: string, rootPath: string, options?: Partial<SearchOptions>) => Promise<SearchResult[]>
  replaceInFiles: (
    query: string,
    replacement: string,
    rootPath: string,
    options?: Partial<SearchOptions>,
    targetPath?: string
  ) => Promise<{ success: boolean; count: number; error?: string }>
  getAllFiles: (rootPath: string) => Promise<string[]>
}

declare global {
  interface Window {
    electronAPI: ElectronAPI
  }
}
