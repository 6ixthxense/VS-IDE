import type {
  AppInfo,
  DiagnosticsSnapshot,
  ElectronAPI,
  GitDiffRequest,
  OperationResult,
  PathOperationResult,
  RunCodePayload,
  SearchOptions,
  SearchResult,
  SysStats,
  TerminalOutputEvent,
  TerminalStatusEvent,
  UpdateStatusEvent,
  WorkspaceChangedEvent,
} from '@shared/types/ipc'
import type { FileEntry } from '@shared/types/file'
import type {
  SqlBrowseTableRequest,
  SqlDeleteRowRequest,
  SqlInsertRowRequest,
  SqlMutationResult,
  SqlQueryRequest,
  SqlQueryResult,
  SqlSchemaSummary,
  SqlServiceStatus,
  SqlTableRowsResult,
  SqlUpdateRowRequest,
} from '@shared/types/sql'

function failedOperation(error: string, details: string): OperationResult {
  return { success: false, error, details }
}

function failedPathOperation(error: string, details: string): PathOperationResult {
  return { success: false, error, details }
}

function noopUnsubscribe() {
  return () => {}
}

function unavailableDetails() {
  return 'Electron preload bridge is unavailable. Start the app through Electron instead of opening the renderer directly in a browser.'
}

function emptyDiagnostics(): DiagnosticsSnapshot {
  return {
    generatedAt: new Date().toISOString(),
    userDataPath: '',
    logFilePath: '',
    entries: [],
  }
}

function emptySqlStatus(): SqlServiceStatus {
  return {
    phase: 'scaffold',
    supportedDialects: ['sqlite', 'postgres', 'mysql'],
    registeredDialects: [],
    connections: [],
    features: {
      editor: true,
      queryRunner: false,
      schemaBrowser: false,
      rowEditing: false,
      savedQueries: false,
      explainPlan: false,
    },
    message: unavailableDetails(),
  }
}

function unavailableSqlResult(connectionId: string, details: string): SqlQueryResult {
  return {
    connectionId,
    success: false,
    columns: [],
    rows: [],
    rowCount: 0,
    durationMs: 0,
    error: details,
  }
}

function unavailableSqlSchema(connectionId: string, details: string): SqlSchemaSummary {
  return {
    connectionId,
    tables: [],
    columnsByTable: {},
    error: details,
  }
}

function unavailableSqlTableRows(connectionId: string, tableId: string, details: string): SqlTableRowsResult {
  return {
    connectionId,
    tableId,
    columns: [],
    rows: [],
    writable: false,
    totalRows: 0,
    error: details,
  }
}

function unavailableSqlMutation(connectionId: string, tableId: string, details: string): SqlMutationResult {
  return {
    connectionId,
    tableId,
    success: false,
    rowCount: 0,
    error: details,
  }
}

export function createElectronApiFallback(): ElectronAPI {
  const details = unavailableDetails()

  return {
    openFolder: async () => null,
    readDir: async (_path: string): Promise<FileEntry[]> => [],
    readFile: async (_path: string) => '',
    writeFile: async (_path: string, _content: string) => failedOperation('Unable to save file', details),
    createFile: async (_dirPath: string, _fileName: string) => failedPathOperation('Unable to create file', details),
    createDirectory: async (_dirPath: string, _folderName: string) => failedPathOperation('Unable to create folder', details),
    deletePath: async (_targetPath: string) => failedOperation('Unable to delete path', details),
    renamePath: async (_oldPath: string, _newPath: string) => failedPathOperation('Unable to rename path', details),
    movePath: async (_oldPath: string, _newPath: string) => failedPathOperation('Unable to move path', details),
    revealInExplorer: (_path: string) => {},
    isWindows: navigator.platform.toLowerCase().includes('win'),
    pathExists: async (_path: string) => false,
    getWorkspace: async () => '',
    setWorkspaceRoot: async (_path: string | null) => null,
    runCode: (_payload: RunCodePayload) => {},
    onSysStats: (_cb: (stats: SysStats) => void) => noopUnsubscribe(),
    onWorkspaceChanged: (_cb: (event: WorkspaceChangedEvent) => void) => noopUnsubscribe(),
    sendTerminalInput: (_id: string, _data: string) => {},
    resizeTerminal: (_id: string, _cols: number, _rows: number) => {},
    setTerminalCwd: (_id: string, _path: string) => {},
    createTerminal: (_id: string, _rootPath: string) => {},
    closeTerminal: (_id: string) => {},
    onTerminalOut: (_cb: (data: TerminalOutputEvent) => void) => noopUnsubscribe(),
    onTerminalStatus: (_cb: (data: TerminalStatusEvent) => void) => noopUnsubscribe(),
    getGitStatus: async (_rootPath: string) => ({}),
    gitAdd: async (_rootPath: string, _filePaths: string[]) => failedOperation('Unable to stage changes', details),
    gitUnstage: async (_rootPath: string, _filePaths: string[]) => failedOperation('Unable to unstage changes', details),
    gitDiscard: async (_rootPath: string, _request: GitDiffRequest) => failedOperation('Unable to discard changes', details),
    gitCommit: async (_rootPath: string, _message: string) => failedOperation('Commit failed', details),
    gitPush: async (_rootPath: string) => failedOperation('Push failed', details),
    gitPull: async (_rootPath: string) => failedOperation('Pull failed', details),
    getGitDiff: async (_rootPath: string, _request: GitDiffRequest) => 'Electron preload bridge unavailable.',
    searchFiles: async (_query: string, _rootPath: string, _options?: Partial<SearchOptions>): Promise<SearchResult[]> => [],
    replaceInFiles: async (_query: string, _replacement: string, _rootPath: string, _options?: Partial<SearchOptions>) => ({
      success: false,
      count: 0,
      error: 'Replace failed',
    }),
    getAllFiles: async (_rootPath: string) => [],
    getAppInfo: async (): Promise<AppInfo> => ({
      version: 'renderer-only',
      isPackaged: false,
      updateConfigured: false,
      bridgeAvailable: false,
    }),
    getDiagnostics: async () => emptyDiagnostics(),
    clearDiagnostics: async () => failedOperation('Unable to clear diagnostics', details),
    openLogFolder: async () => failedOperation('Unable to open diagnostics folder', details),
    checkForUpdates: async () => failedOperation('Unable to check for updates', details),
    installUpdate: async () => failedOperation('Unable to install update', details),
    onUpdateStatus: (_cb: (event: UpdateStatusEvent) => void) => noopUnsubscribe(),
    getSqlStatus: async (): Promise<SqlServiceStatus> => emptySqlStatus(),
    executeSqlQuery: async (request: SqlQueryRequest) => unavailableSqlResult(request.connectionId, details),
    getSqlSchema: async (connectionId: string) => unavailableSqlSchema(connectionId, details),
    getSqlTableRows: async (request: SqlBrowseTableRequest) => unavailableSqlTableRows(request.connectionId, request.tableId, details),
    updateSqlRow: async (request: SqlUpdateRowRequest) => unavailableSqlMutation(request.connectionId, request.tableId, details),
    insertSqlRow: async (request: SqlInsertRowRequest) => unavailableSqlMutation(request.connectionId, request.tableId, details),
    deleteSqlRow: async (request: SqlDeleteRowRequest) => unavailableSqlMutation(request.connectionId, request.tableId, details),
  }
}

export function ensureElectronApi() {
  if (!window.electronAPI) {
    window.electronAPI = createElectronApiFallback()
  }

  return window.electronAPI
}
