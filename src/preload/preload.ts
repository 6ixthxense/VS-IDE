import { contextBridge, ipcRenderer } from 'electron'
import type { GitDiffRequest, RunCodePayload, SearchOptions, SysStats, TerminalOutputEvent, WorkspaceChangedEvent } from '../shared/types/ipc'

const IPC = {
  DIALOG_OPEN_FOLDER: 'dialog:openFolder',
  FS_READ_DIR: 'fs:readDir',
  FS_READ_FILE: 'fs:readFile',
  FS_WRITE_FILE: 'fs:writeFile',
  FS_CREATE_FILE: 'fs:createFile',
  FS_CREATE_DIR: 'fs:createDir',
  FS_DELETE_PATH: 'fs:deletePath',
  FS_PATH_EXISTS: 'fs:pathExists',
  WORKSPACE_GET: 'workspace:get',
  WORKSPACE_SET: 'workspace:set',
  RUN_CODE: 'run-code',
  TERMINAL_OUT: 'terminal-out',
  SYS_STATS: 'sys-stats',
  WORKSPACE_CHANGED: 'workspace:changed',
  FS_RENAME_PATH: 'fs:renamePath',
  FS_MOVE_PATH: 'fs:movePath',
  GIT_STATUS: 'git:status',
  GIT_ADD: 'git:add',
  GIT_UNSTAGE: 'git:unstage',
  GIT_DISCARD: 'git:discard',
  GIT_COMMIT: 'git:commit',
  GIT_PUSH: 'git:push',
  GIT_PULL: 'git:pull',
} as const

contextBridge.exposeInMainWorld('electronAPI', {
  openFolder: (): Promise<string | null> =>
    ipcRenderer.invoke(IPC.DIALOG_OPEN_FOLDER),

  readDir: (path: string) =>
    ipcRenderer.invoke(IPC.FS_READ_DIR, path),

  readFile: (path: string): Promise<string> =>
    ipcRenderer.invoke(IPC.FS_READ_FILE, path),

  writeFile: (path: string, content: string): Promise<boolean> =>
    ipcRenderer.invoke(IPC.FS_WRITE_FILE, path, content),

  createFile: (dirPath: string, fileName: string) =>
    ipcRenderer.invoke(IPC.FS_CREATE_FILE, dirPath, fileName),

  createDirectory: (dirPath: string, folderName: string) =>
    ipcRenderer.invoke(IPC.FS_CREATE_DIR, dirPath, folderName),

  deletePath: (targetPath: string) =>
    ipcRenderer.invoke(IPC.FS_DELETE_PATH, targetPath),

  pathExists: (path: string): Promise<boolean> =>
    ipcRenderer.invoke(IPC.FS_PATH_EXISTS, path),

  getWorkspace: (): Promise<string> =>
    ipcRenderer.invoke(IPC.WORKSPACE_GET),

  setWorkspaceRoot: (path: string | null): Promise<string | null> =>
    ipcRenderer.invoke(IPC.WORKSPACE_SET, path),

  runCode: (payload: RunCodePayload): void =>
    ipcRenderer.send(IPC.RUN_CODE, payload),

  sendTerminalInput: (id: string, data: string) =>
    ipcRenderer.send('terminal:input', id, data),

  resizeTerminal: (id: string, cols: number, rows: number) =>
    ipcRenderer.send('terminal:resize', id, cols, rows),

  setTerminalCwd: (id: string, path: string) =>
    ipcRenderer.send('terminal:set-cwd', id, path),

  createTerminal: (id: string, rootPath: string) =>
    ipcRenderer.send('terminal:create', id, rootPath),

  closeTerminal: (id: string) =>
    ipcRenderer.send('terminal:close', id),

  onTerminalOut: (cb: (data: TerminalOutputEvent) => void) => {
    const listener = (_event: any, data: TerminalOutputEvent) => cb(data)
    ipcRenderer.on(IPC.TERMINAL_OUT, listener)
    return () => ipcRenderer.off(IPC.TERMINAL_OUT, listener)
  },

  onSysStats: (cb: (stats: SysStats) => void) => {
    const handler = (_: Electron.IpcRendererEvent, stats: SysStats) => cb(stats)
    ipcRenderer.on(IPC.SYS_STATS, handler)
    return () => ipcRenderer.removeListener(IPC.SYS_STATS, handler)
  },

  onWorkspaceChanged: (cb: (event: WorkspaceChangedEvent) => void) => {
    const handler = (_: Electron.IpcRendererEvent, event: WorkspaceChangedEvent) => cb(event)
    ipcRenderer.on(IPC.WORKSPACE_CHANGED, handler)
    return () => ipcRenderer.removeListener(IPC.WORKSPACE_CHANGED, handler)
  },

  searchFiles: (query: string, rootPath: string, options?: Partial<SearchOptions>) =>
    ipcRenderer.invoke('ws:search-files', query, rootPath, options),

  replaceInFiles: (
    query: string,
    replacement: string,
    rootPath: string,
    options?: Partial<SearchOptions>,
    targetPath?: string
  ) => ipcRenderer.invoke('ws:replace-in-files', query, replacement, rootPath, options, targetPath),

  renamePath: (oldPath: string, newPath: string) =>
    ipcRenderer.invoke(IPC.FS_RENAME_PATH, oldPath, newPath),

  movePath: (oldPath: string, newPath: string) =>
    ipcRenderer.invoke(IPC.FS_MOVE_PATH, oldPath, newPath),

  revealInExplorer: (path: string) =>
    ipcRenderer.send('shell:reveal', path),

  getGitStatus: (rootPath: string) =>
    ipcRenderer.invoke(IPC.GIT_STATUS, rootPath),

  gitAdd: (rootPath: string, filePaths: string[]) =>
    ipcRenderer.invoke(IPC.GIT_ADD, rootPath, filePaths),

  gitUnstage: (rootPath: string, filePaths: string[]) =>
    ipcRenderer.invoke(IPC.GIT_UNSTAGE, rootPath, filePaths),

  gitDiscard: (rootPath: string, request: GitDiffRequest) =>
    ipcRenderer.invoke(IPC.GIT_DISCARD, rootPath, request),

  gitCommit: (rootPath: string, message: string) =>
    ipcRenderer.invoke(IPC.GIT_COMMIT, rootPath, message),

  gitPush: (rootPath: string) =>
    ipcRenderer.invoke(IPC.GIT_PUSH, rootPath),

  gitPull: (rootPath: string) =>
    ipcRenderer.invoke(IPC.GIT_PULL, rootPath),

  getGitDiff: (rootPath: string, request: GitDiffRequest) =>
    ipcRenderer.invoke('git:diff', rootPath, request),

  getAllFiles: (rootPath: string) =>
    ipcRenderer.invoke('ws:get-all-files', rootPath),

  isWindows: process.platform === 'win32',
})
