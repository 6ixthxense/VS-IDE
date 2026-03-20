import { contextBridge, ipcRenderer } from 'electron'
import { IPC } from '../shared/constants'
import type { AppInfo, DiagnosticsSnapshot, ElectronAPI, GitDiffRequest, GitStatus, OperationResult, PathOperationResult, ProblemScanResult, RunCodePayload, SearchOptions, SearchResult, SysStats, TaskDefinition, TaskEvent, TaskRun, TaskRunRequest, TerminalOutputEvent, TerminalStatusEvent, UpdateStatusEvent, WorkspaceChangedEvent } from '../shared/types/ipc'
import type { FileEntry } from '../shared/types/file'
import type { SqlBrowseTableRequest, SqlDeleteRowRequest, SqlInsertRowRequest, SqlMutationResult, SqlQueryRequest, SqlQueryResult, SqlSchemaSummary, SqlServiceStatus, SqlTableRowsResult, SqlUpdateRowRequest } from '../shared/types/sql'
import { normalizeGitDiffRequest, normalizeRunCodePayload, normalizeSearchOptions, normalizeSqlBrowseTableRequest, normalizeSqlDeleteRowRequest, normalizeSqlInsertRowRequest, normalizeSqlQueryRequest, normalizeSqlUpdateRowRequest, normalizeTaskRunRequest, requireCallback, requireIdentifier, requireMaybeEmptyString, requirePath } from './validation'

function invoke<T>(channel: string, ...args: unknown[]): Promise<T> {
  return ipcRenderer.invoke(channel, ...args) as Promise<T>
}

function send(channel: string, ...args: unknown[]): void {
  ipcRenderer.send(channel, ...args)
}

function subscribe<T>(channel: string, callback: unknown) {
  const safeCallback = requireCallback<T>(callback, 'IPC callback')
  const listener = (_event: Electron.IpcRendererEvent, payload: T) => {
    safeCallback(payload)
  }

  ipcRenderer.on(channel, listener)
  return () => ipcRenderer.removeListener(channel, listener)
}

const api: ElectronAPI = Object.freeze({
  openFolder: (): Promise<string | null> =>
    invoke<string | null>(IPC.DIALOG_OPEN_FOLDER),

  readDir: (path: string): Promise<FileEntry[]> =>
    invoke<FileEntry[]>(IPC.FS_READ_DIR, requirePath(path)),

  readFile: (path: string): Promise<string> =>
    invoke<string>(IPC.FS_READ_FILE, requirePath(path)),

  writeFile: (path: string, content: string): Promise<OperationResult> =>
    invoke<OperationResult>(IPC.FS_WRITE_FILE, requirePath(path), content),

  createFile: (dirPath: string, fileName: string) =>
    invoke<PathOperationResult>(
      IPC.FS_CREATE_FILE,
      requirePath(dirPath, 'Directory path'),
      requireIdentifier(fileName, 'File name')
    ),

  createDirectory: (dirPath: string, folderName: string) =>
    invoke<PathOperationResult>(
      IPC.FS_CREATE_DIR,
      requirePath(dirPath, 'Directory path'),
      requireIdentifier(folderName, 'Folder name')
    ),

  deletePath: (targetPath: string) =>
    invoke<OperationResult>(IPC.FS_DELETE_PATH, requirePath(targetPath, 'Target path')),

  pathExists: (path: string): Promise<boolean> =>
    invoke<boolean>(IPC.FS_PATH_EXISTS, requirePath(path)),

  getWorkspace: (): Promise<string> =>
    invoke<string>(IPC.WORKSPACE_GET),

  setWorkspaceRoot: (path: string | null): Promise<string | null> =>
    invoke<string | null>(IPC.WORKSPACE_SET, path === null ? null : requirePath(path)),

  runCode: (payload: RunCodePayload): void =>
    send(IPC.RUN_CODE, normalizeRunCodePayload(payload)),

  sendTerminalInput: (id: string, data: string) =>
    send(IPC.TERMINAL_INPUT, requireIdentifier(id, 'Terminal id'), data),

  resizeTerminal: (id: string, cols: number, rows: number) =>
    send(IPC.TERMINAL_RESIZE, requireIdentifier(id, 'Terminal id'), cols, rows),

  setTerminalCwd: (id: string, path: string) =>
    send(IPC.TERMINAL_SET_CWD, requireIdentifier(id, 'Terminal id'), requirePath(path)),

  createTerminal: (id: string, rootPath: string) =>
    send(IPC.TERMINAL_CREATE, requireIdentifier(id, 'Terminal id'), requireMaybeEmptyString(rootPath, 'Terminal root path')),

  closeTerminal: (id: string) =>
    send(IPC.TERMINAL_CLOSE, requireIdentifier(id, 'Terminal id')),

  onTerminalOut: (cb: (data: TerminalOutputEvent) => void) =>
    subscribe<TerminalOutputEvent>(IPC.TERMINAL_OUT, cb),

  onTerminalStatus: (cb: (data: TerminalStatusEvent) => void) =>
    subscribe<TerminalStatusEvent>(IPC.TERMINAL_STATUS, cb),

  onSysStats: (cb: (stats: SysStats) => void) =>
    subscribe<SysStats>(IPC.SYS_STATS, cb),

  onWorkspaceChanged: (cb: (event: WorkspaceChangedEvent) => void) =>
    subscribe<WorkspaceChangedEvent>(IPC.WORKSPACE_CHANGED, cb),

  searchFiles: (query: string, rootPath: string, options?: Partial<SearchOptions>) =>
    invoke<SearchResult[]>(
      IPC.WORKSPACE_SEARCH_FILES,
      requireIdentifier(query, 'Search query'),
      requirePath(rootPath, 'Workspace root path'),
      normalizeSearchOptions(options)
    ),

  replaceInFiles: (
    query: string,
    replacement: string,
    rootPath: string,
    options?: Partial<SearchOptions>,
    targetPath?: string
  ) => invoke<{ success: boolean; count: number; error?: string }>(
    IPC.WORKSPACE_REPLACE_IN_FILES,
    requireIdentifier(query, 'Search query'),
    replacement,
    requirePath(rootPath, 'Workspace root path'),
    normalizeSearchOptions(options),
    targetPath == null ? undefined : requirePath(targetPath, 'Target path')
  ),

  renamePath: (oldPath: string, newPath: string) =>
    invoke<PathOperationResult>(
      IPC.FS_RENAME_PATH,
      requirePath(oldPath, 'Source path'),
      requirePath(newPath, 'Destination path')
    ),

  movePath: (oldPath: string, newPath: string) =>
    invoke<PathOperationResult>(
      IPC.FS_MOVE_PATH,
      requirePath(oldPath, 'Source path'),
      requirePath(newPath, 'Destination path')
    ),

  revealInExplorer: (path: string) =>
    send(IPC.SHELL_REVEAL, requirePath(path)),

  getGitStatus: (rootPath: string) =>
    invoke<GitStatus>(IPC.GIT_STATUS, requirePath(rootPath, 'Workspace root path')),

  gitAdd: (rootPath: string, filePaths: string[]) =>
    invoke<OperationResult>(IPC.GIT_ADD, requirePath(rootPath, 'Workspace root path'), filePaths.map((filePath) => requirePath(filePath))),

  gitUnstage: (rootPath: string, filePaths: string[]) =>
    invoke<OperationResult>(IPC.GIT_UNSTAGE, requirePath(rootPath, 'Workspace root path'), filePaths.map((filePath) => requirePath(filePath))),

  gitDiscard: (rootPath: string, request: GitDiffRequest) =>
    invoke<OperationResult>(IPC.GIT_DISCARD, requirePath(rootPath, 'Workspace root path'), normalizeGitDiffRequest(request)),

  gitCommit: (rootPath: string, message: string) =>
    invoke<OperationResult>(IPC.GIT_COMMIT, requirePath(rootPath, 'Workspace root path'), requireIdentifier(message, 'Commit message')),

  gitPush: (rootPath: string) =>
    invoke<OperationResult>(IPC.GIT_PUSH, requirePath(rootPath, 'Workspace root path')),

  gitPull: (rootPath: string) =>
    invoke<OperationResult>(IPC.GIT_PULL, requirePath(rootPath, 'Workspace root path')),

  getGitDiff: (rootPath: string, request: GitDiffRequest) =>
    invoke<string>(IPC.GIT_DIFF, requirePath(rootPath, 'Workspace root path'), normalizeGitDiffRequest(request)),

  scanProblems: (rootPath: string) =>
    invoke<ProblemScanResult>(IPC.PROBLEMS_SCAN, requirePath(rootPath, 'Workspace root path')),

  getLastProblems: (rootPath: string) =>
    invoke<ProblemScanResult>(IPC.PROBLEMS_GET_LAST, requirePath(rootPath, 'Workspace root path')),

  getTaskDefinitions: (rootPath: string) =>
    invoke<TaskDefinition[]>(IPC.TASKS_LIST, requirePath(rootPath, 'Workspace root path')),

  runTask: (rootPath: string, request: TaskRunRequest) =>
    invoke<OperationResult & { run?: TaskRun }>(IPC.TASKS_RUN, requirePath(rootPath, 'Workspace root path'), normalizeTaskRunRequest(request)),

  stopTask: (runId: string) =>
    invoke<OperationResult>(IPC.TASKS_STOP, requireIdentifier(runId, 'Task run id')),

  onTaskEvent: (cb: (event: TaskEvent) => void) =>
    subscribe<TaskEvent>(IPC.TASKS_EVENT, cb),

  getAllFiles: (rootPath: string) =>
    invoke<string[]>(IPC.WORKSPACE_GET_ALL_FILES, requirePath(rootPath, 'Workspace root path')),

  getAppInfo: () =>
    invoke<AppInfo>(IPC.APP_INFO),

  getDiagnostics: () =>
    invoke<DiagnosticsSnapshot>(IPC.APP_GET_DIAGNOSTICS),

  clearDiagnostics: () =>
    invoke<OperationResult>(IPC.APP_CLEAR_DIAGNOSTICS),

  openLogFolder: () =>
    invoke<OperationResult>(IPC.APP_OPEN_LOG_FOLDER),

  checkForUpdates: () =>
    invoke<OperationResult>(IPC.APP_CHECK_FOR_UPDATES),

  installUpdate: () =>
    invoke<OperationResult>(IPC.APP_INSTALL_UPDATE),

  onUpdateStatus: (cb: (event: UpdateStatusEvent) => void) =>
    subscribe<UpdateStatusEvent>(IPC.APP_UPDATE_STATUS, cb),

  getSqlStatus: () =>
    invoke<SqlServiceStatus>(IPC.SQL_GET_STATUS),

  executeSqlQuery: (request: SqlQueryRequest) =>
    invoke<SqlQueryResult>(IPC.SQL_EXECUTE_QUERY, normalizeSqlQueryRequest(request)),

  getSqlSchema: (connectionId: string) =>
    invoke<SqlSchemaSummary>(IPC.SQL_GET_SCHEMA, requireIdentifier(connectionId, 'SQL connection id')),

  getSqlTableRows: (request: SqlBrowseTableRequest) =>
    invoke<SqlTableRowsResult>(IPC.SQL_GET_TABLE_ROWS, normalizeSqlBrowseTableRequest(request)),

  updateSqlRow: (request: SqlUpdateRowRequest) =>
    invoke<SqlMutationResult>(IPC.SQL_UPDATE_ROW, normalizeSqlUpdateRowRequest(request)),

  insertSqlRow: (request: SqlInsertRowRequest) =>
    invoke<SqlMutationResult>(IPC.SQL_INSERT_ROW, normalizeSqlInsertRowRequest(request)),

  deleteSqlRow: (request: SqlDeleteRowRequest) =>
    invoke<SqlMutationResult>(IPC.SQL_DELETE_ROW, normalizeSqlDeleteRowRequest(request)),

  isWindows: process.platform === 'win32',
})

contextBridge.exposeInMainWorld('electronAPI', api)
