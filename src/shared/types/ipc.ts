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

export interface TerminalStatusEvent {
  id: string
  level: 'info' | 'warning' | 'error'
  title: string
  message: string
  details?: string
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

export interface OperationResult {
  success: boolean
  error?: string
  details?: string
}

export interface PathOperationResult extends OperationResult {
  path?: string
}

export interface AppInfo {
  version: string
  isPackaged: boolean
  updateConfigured: boolean
  bridgeAvailable: boolean
  updateChannel?: string
  updateSource?: string
}

export interface UpdateStatusEvent {
  state: 'idle' | 'checking' | 'available' | 'not-available' | 'downloading' | 'downloaded' | 'error'
  message: string
  version?: string
  progress?: number
  details?: string
}

export interface DiagnosticEntry {
  id: string
  timestamp: string
  level: 'info' | 'warning' | 'error'
  source: string
  message: string
  details?: string
}

export interface DiagnosticsSnapshot {
  generatedAt: string
  userDataPath: string
  logFilePath: string
  entries: DiagnosticEntry[]
}

export interface ElectronAPI {
  openFolder: () => Promise<string | null>
  readDir: (path: string) => Promise<FileEntry[]>
  readFile: (path: string) => Promise<string>
  writeFile: (path: string, content: string) => Promise<OperationResult>
  createFile: (dirPath: string, fileName: string) => Promise<PathOperationResult>
  createDirectory: (dirPath: string, folderName: string) => Promise<PathOperationResult>
  deletePath: (targetPath: string) => Promise<OperationResult>
  renamePath: (oldPath: string, newPath: string) => Promise<PathOperationResult>
  movePath: (oldPath: string, newPath: string) => Promise<PathOperationResult>
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
  onTerminalStatus: (cb: (data: TerminalStatusEvent) => void) => () => void
  getGitStatus: (rootPath: string) => Promise<GitStatus>
  gitAdd: (rootPath: string, filePaths: string[]) => Promise<OperationResult>
  gitUnstage: (rootPath: string, filePaths: string[]) => Promise<OperationResult>
  gitDiscard: (rootPath: string, request: GitDiffRequest) => Promise<OperationResult>
  gitCommit: (rootPath: string, message: string) => Promise<OperationResult>
  gitPush: (rootPath: string) => Promise<OperationResult>
  gitPull: (rootPath: string) => Promise<OperationResult>
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
  getAppInfo: () => Promise<AppInfo>
  getDiagnostics: () => Promise<DiagnosticsSnapshot>
  clearDiagnostics: () => Promise<OperationResult>
  openLogFolder: () => Promise<OperationResult>
  checkForUpdates: () => Promise<OperationResult>
  installUpdate: () => Promise<OperationResult>
  onUpdateStatus: (cb: (event: UpdateStatusEvent) => void) => () => void
}

declare global {
  interface Window {
    electronAPI: ElectronAPI
  }
}
