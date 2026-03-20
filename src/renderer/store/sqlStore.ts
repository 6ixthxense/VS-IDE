import { create } from 'zustand'
import type {
  SqlBrowseFilter,
  SqlFilterOperator,
  SqlMutationResult,
  SqlQueryResult,
  SqlRowLocator,
  SqlSchemaSummary,
  SqlServiceStatus,
  SqlTableRowsResult,
  SqlTableSummary,
} from '@shared/types/sql'
import { showErrorToast, showSuccessToast, showWarningToast } from './feedbackStore'

interface WorkspaceDatabase {
  id: string
  name: string
  path: string
}

interface SqlBrowseState {
  pageSize: number
  pageIndex: number
  sortColumn: string | null
  sortDirection: 'asc' | 'desc'
  filters: SqlBrowseFilter[]
}

export interface SqlQueryHistoryEntry {
  id: string
  sql: string
  connectionId: string
  executedAt: string
  success: boolean
  rowCount: number
  durationMs: number
}

export interface SqlSavedSnippet {
  id: string
  name: string
  sql: string
  createdAt: string
  updatedAt: string
}

interface SqlState {
  serviceStatus: SqlServiceStatus | null
  databaseFiles: WorkspaceDatabase[]
  activeConnectionId: string | null
  activeTableId: string | null
  schema: SqlSchemaSummary | null
  queryResult: SqlQueryResult | null
  tableRows: SqlTableRowsResult | null
  resultMode: 'browser' | 'query'
  queryText: string
  queryHistory: SqlQueryHistoryEntry[]
  savedSnippets: SqlSavedSnippet[]
  browse: SqlBrowseState
  browseByTable: Record<string, SqlBrowseState>
  isLoadingFiles: boolean
  isLoadingSchema: boolean
  isLoadingTableRows: boolean
  isRunningQuery: boolean
  isMutatingRows: boolean
  refreshStatus: () => Promise<void>
  refreshWorkspaceDatabases: (rootPath: string | null) => Promise<void>
  selectConnection: (connectionId: string) => Promise<void>
  refreshSchema: () => Promise<void>
  loadTableRows: (tableId?: string) => Promise<void>
  setBrowsePage: (pageIndex: number) => Promise<void>
  setBrowsePageSize: (pageSize: number) => Promise<void>
  setBrowseSort: (sortColumn: string | null, sortDirection?: 'asc' | 'desc') => Promise<void>
  setBrowseFilters: (filters: SqlBrowseFilter[]) => Promise<void>
  applyBrowseState: (browseState: Partial<SqlBrowseState>) => Promise<void>
  setQueryText: (queryText: string) => void
  saveSnippet: (name: string, sqlText?: string) => SqlSavedSnippet | null
  updateSnippet: (snippetId: string, name: string, sqlText?: string) => SqlSavedSnippet | null
  deleteSnippet: (snippetId: string) => void
  clearQueryHistory: () => void
  runQuery: (queryTextOverride?: string) => Promise<SqlQueryResult | null>
  browseTable: (table: SqlTableSummary) => Promise<void>
  updateTableRow: (locator: SqlRowLocator, values: Record<string, unknown>) => Promise<SqlMutationResult | null>
  insertTableRow: (values: Record<string, unknown>) => Promise<SqlMutationResult | null>
  deleteTableRow: (locator: SqlRowLocator) => Promise<SqlMutationResult | null>
  reset: () => void
}

const SQLITE_FILE_EXTENSIONS = new Set(['db', 'db3', 'sqlite', 'sqlite3'])
const DEFAULT_PAGE_SIZE = 25
const DATABASE_BROWSE_STATE_KEY = 'vs-monitor-ide:database:browse-state'
const DATABASE_QUERY_HISTORY_KEY = 'vs-monitor-ide:database:query-history'
const DATABASE_SAVED_SNIPPETS_KEY = 'vs-monitor-ide:database:saved-snippets'
const DEFAULT_QUERY = [
  'SELECT name, type',
  'FROM sqlite_master',
  "WHERE type IN ('table', 'view')",
  'ORDER BY type, name;',
].join('\n')
const VALID_FILTER_OPERATORS = new Set<SqlFilterOperator>([
  'contains',
  'startsWith',
  'endsWith',
  'equals',
  'notEquals',
  'greaterThan',
  'greaterThanOrEqual',
  'lessThan',
  'lessThanOrEqual',
  'isNull',
  'isNotNull',
])

function createDefaultBrowseFilter(operator: SqlFilterOperator = 'contains'): SqlBrowseFilter {
  return { operator, value: '' }
}

function cloneBrowseFilters(filters: SqlBrowseFilter[]) {
  return filters.map((filter) => ({
    column: filter.column,
    operator: filter.operator,
    value: filter.value ?? '',
  }))
}

function createDefaultBrowseState(): SqlBrowseState {
  return {
    pageSize: DEFAULT_PAGE_SIZE,
    pageIndex: 0,
    sortColumn: null,
    sortDirection: 'asc',
    filters: [createDefaultBrowseFilter()],
  }
}

function cloneBrowseState(browse: SqlBrowseState): SqlBrowseState {
  return {
    pageSize: browse.pageSize,
    pageIndex: browse.pageIndex,
    sortColumn: browse.sortColumn,
    sortDirection: browse.sortDirection,
    filters: cloneBrowseFilters(browse.filters),
  }
}

function sanitizeBrowseFilter(filter: unknown): SqlBrowseFilter | null {
  if (!filter || typeof filter !== 'object') return null

  const candidate = filter as Record<string, unknown>
  const operator = candidate.operator
  if (typeof operator !== 'string' || !VALID_FILTER_OPERATORS.has(operator as SqlFilterOperator)) {
    return null
  }

  return {
    column: typeof candidate.column === 'string' && candidate.column.trim().length > 0 ? candidate.column : undefined,
    operator: operator as SqlFilterOperator,
    value: typeof candidate.value === 'string' ? candidate.value : '',
  }
}

function sanitizeBrowseState(value: unknown): SqlBrowseState {
  if (!value || typeof value !== 'object') {
    return createDefaultBrowseState()
  }

  const candidate = value as Record<string, unknown>
  const pageSizeCandidate = typeof candidate.pageSize === 'number' ? candidate.pageSize : DEFAULT_PAGE_SIZE
  const pageSize = [10, 25, 50, 100].includes(pageSizeCandidate) ? pageSizeCandidate : DEFAULT_PAGE_SIZE
  const pageIndex = typeof candidate.pageIndex === 'number' && Number.isFinite(candidate.pageIndex)
    ? Math.max(0, Math.floor(candidate.pageIndex))
    : 0
  const sortColumn = typeof candidate.sortColumn === 'string' && candidate.sortColumn.trim().length > 0 ? candidate.sortColumn : null
  const sortDirection = candidate.sortDirection === 'desc' ? 'desc' : 'asc'
  const filters = Array.isArray(candidate.filters)
    ? candidate.filters.map(sanitizeBrowseFilter).filter((filter): filter is SqlBrowseFilter => filter != null)
    : []

  return {
    pageSize,
    pageIndex,
    sortColumn,
    sortDirection,
    filters: filters.length > 0 ? filters : [createDefaultBrowseFilter()],
  }
}

function readPersistedBrowseByTable() {
  if (typeof window === 'undefined') return {}

  try {
    const raw = window.localStorage.getItem(DATABASE_BROWSE_STATE_KEY)
    if (!raw) return {}

    const parsed = JSON.parse(raw) as Record<string, unknown>
    return Object.fromEntries(
      Object.entries(parsed).map(([key, value]) => [key, sanitizeBrowseState(value)])
    ) as Record<string, SqlBrowseState>
  } catch {
    return {}
  }
}

function writePersistedBrowseByTable(browseByTable: Record<string, SqlBrowseState>) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_BROWSE_STATE_KEY, JSON.stringify(browseByTable))
  } catch {
    /* ignore */
  }
}

function sanitizeQueryHistoryEntry(value: unknown): SqlQueryHistoryEntry | null {
  if (!value || typeof value !== 'object') return null

  const candidate = value as Record<string, unknown>
  const id = typeof candidate.id === 'string' && candidate.id.trim().length > 0 ? candidate.id : null
  const sql = typeof candidate.sql === 'string' ? candidate.sql.trim() : ''
  const connectionId = typeof candidate.connectionId === 'string' && candidate.connectionId.trim().length > 0 ? candidate.connectionId : null
  const executedAt = typeof candidate.executedAt === 'string' && candidate.executedAt.trim().length > 0
    ? candidate.executedAt
    : new Date().toISOString()

  if (!id || !connectionId || sql.length === 0) {
    return null
  }

  return {
    id,
    sql,
    connectionId,
    executedAt,
    success: Boolean(candidate.success),
    rowCount: typeof candidate.rowCount === 'number' && Number.isFinite(candidate.rowCount) ? candidate.rowCount : 0,
    durationMs: typeof candidate.durationMs === 'number' && Number.isFinite(candidate.durationMs) ? candidate.durationMs : 0,
  }
}

function readPersistedQueryHistory() {
  if (typeof window === 'undefined') return []

  try {
    const raw = window.localStorage.getItem(DATABASE_QUERY_HISTORY_KEY)
    if (!raw) return []

    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []

    return parsed
      .map(sanitizeQueryHistoryEntry)
      .filter((entry): entry is SqlQueryHistoryEntry => entry != null)
      .slice(0, 40)
  } catch {
    return []
  }
}

function writePersistedQueryHistory(queryHistory: SqlQueryHistoryEntry[]) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_QUERY_HISTORY_KEY, JSON.stringify(queryHistory.slice(0, 40)))
  } catch {
    /* ignore */
  }
}

function rememberQueryHistoryEntry(
  queryHistory: SqlQueryHistoryEntry[],
  entry: Omit<SqlQueryHistoryEntry, 'id'>
) {
  const nextEntry: SqlQueryHistoryEntry = {
    id: `${entry.connectionId}:${Date.now().toString(36)}:${Math.random().toString(36).slice(2, 8)}`,
    ...entry,
  }

  const deduped = queryHistory.filter((historyEntry) => !(
    historyEntry.connectionId === entry.connectionId
    && historyEntry.sql === entry.sql
  ))

  return [nextEntry, ...deduped].slice(0, 40)
}

function sanitizeSavedSnippet(value: unknown): SqlSavedSnippet | null {
  if (!value || typeof value !== 'object') return null

  const candidate = value as Record<string, unknown>
  const id = typeof candidate.id === 'string' && candidate.id.trim().length > 0 ? candidate.id : null
  const name = typeof candidate.name === 'string' ? candidate.name.trim() : ''
  const sql = typeof candidate.sql === 'string' ? candidate.sql.trim() : ''
  const createdAt = typeof candidate.createdAt === 'string' && candidate.createdAt.trim().length > 0
    ? candidate.createdAt
    : new Date().toISOString()
  const updatedAt = typeof candidate.updatedAt === 'string' && candidate.updatedAt.trim().length > 0
    ? candidate.updatedAt
    : createdAt

  if (!id || name.length === 0 || sql.length === 0) {
    return null
  }

  return {
    id,
    name,
    sql,
    createdAt,
    updatedAt,
  }
}

function readPersistedSavedSnippets() {
  if (typeof window === 'undefined') return []

  try {
    const raw = window.localStorage.getItem(DATABASE_SAVED_SNIPPETS_KEY)
    if (!raw) return []

    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []

    return parsed
      .map(sanitizeSavedSnippet)
      .filter((snippet): snippet is SqlSavedSnippet => snippet != null)
      .slice(0, 80)
  } catch {
    return []
  }
}

function writePersistedSavedSnippets(savedSnippets: SqlSavedSnippet[]) {
  if (typeof window === 'undefined') return

  try {
    window.localStorage.setItem(DATABASE_SAVED_SNIPPETS_KEY, JSON.stringify(savedSnippets.slice(0, 80)))
  } catch {
    /* ignore */
  }
}

function createBrowseStateKey(connectionId: string | null, tableId: string | null) {
  return connectionId && tableId ? `${connectionId}::${tableId}` : null
}

function getSavedBrowseState(state: Pick<SqlState, 'browseByTable'>, connectionId: string | null, tableId: string | null) {
  const key = createBrowseStateKey(connectionId, tableId)
  if (!key) {
    return createDefaultBrowseState()
  }

  const saved = state.browseByTable[key]
  return saved ? cloneBrowseState(saved) : createDefaultBrowseState()
}

function persistBrowseState(
  state: Pick<SqlState, 'activeConnectionId' | 'activeTableId' | 'browseByTable'>,
  browse: SqlBrowseState
) {
  const key = createBrowseStateKey(state.activeConnectionId, state.activeTableId)
  if (!key) {
    return {
      browse: cloneBrowseState(browse),
      browseByTable: state.browseByTable,
    }
  }

  return {
    browse: cloneBrowseState(browse),
    browseByTable: {
      ...state.browseByTable,
      [key]: cloneBrowseState(browse),
    },
  }
}

function createSqliteConnectionId(path: string) {
  return `sqlite:${path}`
}

function getDatabaseName(path: string) {
  return path.replace(/\\/g, '/').split('/').pop() ?? path
}

function isSqliteWorkspaceFile(path: string) {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  return SQLITE_FILE_EXTENSIONS.has(ext)
}

export const useSqlStore = create<SqlState>((set, get) => ({
  serviceStatus: null,
  databaseFiles: [],
  activeConnectionId: null,
  activeTableId: null,
  schema: null,
  queryResult: null,
  tableRows: null,
  resultMode: 'browser',
  queryText: DEFAULT_QUERY,
  queryHistory: readPersistedQueryHistory(),
  savedSnippets: readPersistedSavedSnippets(),
  browse: createDefaultBrowseState(),
  browseByTable: readPersistedBrowseByTable(),
  isLoadingFiles: false,
  isLoadingSchema: false,
  isLoadingTableRows: false,
  isRunningQuery: false,
  isMutatingRows: false,

  refreshStatus: async () => {
    const serviceStatus = await window.electronAPI.getSqlStatus()
    set({ serviceStatus })
  },

  refreshWorkspaceDatabases: async (rootPath) => {
    if (!rootPath) {
      set({
        databaseFiles: [],
        activeConnectionId: null,
        activeTableId: null,
        schema: null,
        queryResult: null,
        tableRows: null,
        browse: createDefaultBrowseState(),
        browseByTable: {},
      })
      return
    }

    set({ isLoadingFiles: true })

    try {
      const files = await window.electronAPI.getAllFiles(rootPath)
      const databaseFiles = files
        .filter(isSqliteWorkspaceFile)
        .sort((left, right) => left.localeCompare(right))
        .map<WorkspaceDatabase>((path) => ({
          id: createSqliteConnectionId(path),
          name: getDatabaseName(path),
          path,
        }))

      const nextActiveConnectionId = databaseFiles.some((file) => file.id === get().activeConnectionId)
        ? get().activeConnectionId
        : databaseFiles[0]?.id ?? null

      set({
        databaseFiles,
        activeConnectionId: nextActiveConnectionId,
        activeTableId: null,
        browse: createDefaultBrowseState(),
        isLoadingFiles: false,
      })

      if (nextActiveConnectionId) {
        await get().selectConnection(nextActiveConnectionId)
      } else {
        set({ schema: null, queryResult: null, tableRows: null })
      }
    } catch (error) {
      set({ isLoadingFiles: false })
      showErrorToast((error as Error).message || 'Unable to scan the workspace for SQLite files.', 'Database scan failed')
    }
  },

  selectConnection: async (activeConnectionId) => {
      set({
        activeConnectionId,
        activeTableId: null,
        queryResult: null,
        tableRows: null,
        resultMode: 'browser',
        browse: createDefaultBrowseState(),
      })
    await get().refreshSchema()
  },

  refreshSchema: async () => {
    const activeConnectionId = get().activeConnectionId
    if (!activeConnectionId) return

    set({ isLoadingSchema: true })

    try {
      const schema = await window.electronAPI.getSqlSchema(activeConnectionId)
      const nextActiveTableId = schema.tables.some((table) => table.id === get().activeTableId)
        ? get().activeTableId
        : schema.tables[0]?.id ?? null
      const nextBrowse = getSavedBrowseState(get(), activeConnectionId, nextActiveTableId)

      set({
        schema,
        activeTableId: nextActiveTableId,
        browse: nextBrowse,
        isLoadingSchema: false,
      })

      if (schema.error) {
        showErrorToast(schema.error, 'Schema load failed')
        return
      }

      if (nextActiveTableId) {
        await get().loadTableRows(nextActiveTableId)
      } else {
        set({ tableRows: null })
      }
    } catch (error) {
      set({ isLoadingSchema: false })
      showErrorToast((error as Error).message || 'Unable to load the database schema.', 'Schema load failed')
    }
  },

  loadTableRows: async (tableId) => {
    const activeConnectionId = get().activeConnectionId
    const currentState = get()
    const nextTableId = tableId ?? currentState.activeTableId
    if (!activeConnectionId || !nextTableId) return
    const browse = tableId && tableId !== currentState.activeTableId
      ? getSavedBrowseState(currentState, activeConnectionId, nextTableId)
      : currentState.browse
    const { pageSize, pageIndex, sortColumn, sortDirection, filters } = browse

    set({
      activeTableId: nextTableId,
      browse: cloneBrowseState(browse),
      isLoadingTableRows: true,
      resultMode: 'browser',
    })

    try {
      const tableRows = await window.electronAPI.getSqlTableRows({
        connectionId: activeConnectionId,
        tableId: nextTableId,
        limit: pageSize,
        offset: pageIndex * pageSize,
        sortColumn: sortColumn ?? undefined,
        sortDirection,
        filters: filters.filter((filter) => {
          if (filter.operator === 'isNull' || filter.operator === 'isNotNull') {
            return Boolean(filter.column)
          }

          if (!filter.value || filter.value.trim().length === 0) {
            return false
          }

          return filter.operator === 'contains' || Boolean(filter.column)
        }),
      })

      set({
        tableRows,
        isLoadingTableRows: false,
      })

      if (tableRows.error) {
        showErrorToast(tableRows.error, 'Table browser failed')
      }
    } catch (error) {
      set({ isLoadingTableRows: false })
      showErrorToast((error as Error).message || 'Unable to load table rows.', 'Table browser failed')
    }
  },

  setBrowsePage: async (pageIndex) => {
    set((state) => persistBrowseState(state, {
      ...state.browse,
        pageIndex: Math.max(0, pageIndex),
    }))
    writePersistedBrowseByTable(get().browseByTable)
    await get().loadTableRows()
  },

  setBrowsePageSize: async (pageSize) => {
    set((state) => persistBrowseState(state, {
      ...state.browse,
        pageSize,
        pageIndex: 0,
    }))
    writePersistedBrowseByTable(get().browseByTable)
    await get().loadTableRows()
  },

  setBrowseSort: async (sortColumn, sortDirection) => {
    set((state) => persistBrowseState(state, {
      ...state.browse,
        sortColumn,
        sortDirection: sortDirection ?? state.browse.sortDirection,
        pageIndex: 0,
    }))
    writePersistedBrowseByTable(get().browseByTable)
    await get().loadTableRows()
  },

  setBrowseFilters: async (filters) => {
    set((state) => persistBrowseState(state, {
      ...state.browse,
        filters: cloneBrowseFilters(filters),
        pageIndex: 0,
    }))
    writePersistedBrowseByTable(get().browseByTable)
    await get().loadTableRows()
  },

  applyBrowseState: async (browseState) => {
    set((state) => persistBrowseState(state, sanitizeBrowseState({
      ...state.browse,
      ...browseState,
      filters: browseState.filters ?? state.browse.filters,
      pageIndex: browseState.pageIndex ?? 0,
    })))
    writePersistedBrowseByTable(get().browseByTable)
    await get().loadTableRows()
  },

  setQueryText: (queryText) => set({ queryText }),

  saveSnippet: (name, sqlText) => {
    const snippetName = name.trim()
    const sql = (sqlText ?? get().queryText).trim()

    if (snippetName.length === 0 || sql.length === 0) {
      return null
    }

    const now = new Date().toISOString()
    const snippet: SqlSavedSnippet = {
      id: `snippet:${Date.now().toString(36)}:${Math.random().toString(36).slice(2, 8)}`,
      name: snippetName,
      sql,
      createdAt: now,
      updatedAt: now,
    }

    set((state) => ({
      savedSnippets: [snippet, ...state.savedSnippets.filter((entry) => entry.id !== snippet.id)].slice(0, 80),
    }))
    writePersistedSavedSnippets(get().savedSnippets)
    return snippet
  },

  updateSnippet: (snippetId, name, sqlText) => {
    const snippetName = name.trim()
    const sql = (sqlText ?? get().queryText).trim()
    if (snippetName.length === 0 || sql.length === 0) {
      return null
    }

    const currentSnippet = get().savedSnippets.find((entry) => entry.id === snippetId)
    if (!currentSnippet) {
      return null
    }

    const updatedSnippet: SqlSavedSnippet = {
      ...currentSnippet,
      name: snippetName,
      sql,
      updatedAt: new Date().toISOString(),
    }

    set((state) => ({
      savedSnippets: [
        updatedSnippet,
        ...state.savedSnippets.filter((entry) => entry.id !== snippetId),
      ].slice(0, 80),
    }))
    writePersistedSavedSnippets(get().savedSnippets)
    return updatedSnippet
  },

  deleteSnippet: (snippetId) => {
    set((state) => ({
      savedSnippets: state.savedSnippets.filter((entry) => entry.id !== snippetId),
    }))
    writePersistedSavedSnippets(get().savedSnippets)
  },

  clearQueryHistory: () => {
    set({ queryHistory: [] })
    writePersistedQueryHistory([])
  },

  runQuery: async (queryTextOverride) => {
    const activeConnectionId = get().activeConnectionId
    const sql = queryTextOverride ?? get().queryText
    const normalizedSql = sql.trim()

    if (!activeConnectionId) {
      showWarningToast('Choose a SQLite file from the workspace before running a query.', 'No database selected')
      return null
    }

    if (normalizedSql.length === 0) {
      showWarningToast('Type a SQL statement before running the query.', 'No query to run')
      return null
    }

    set({ isRunningQuery: true })

    try {
      const queryResult = await window.electronAPI.executeSqlQuery({
        connectionId: activeConnectionId,
        sql,
      })

      const nextQueryHistory = rememberQueryHistoryEntry(get().queryHistory, {
        sql: normalizedSql,
        connectionId: activeConnectionId,
        executedAt: new Date().toISOString(),
        success: queryResult.success,
        rowCount: queryResult.rowCount,
        durationMs: queryResult.durationMs,
      })

      set({
        queryResult,
        resultMode: 'query',
        queryHistory: nextQueryHistory,
        isRunningQuery: false,
      })
      writePersistedQueryHistory(nextQueryHistory)

      if (!queryResult.success && queryResult.error) {
        showErrorToast(queryResult.error, 'SQL query failed')
      }

      return queryResult
    } catch (error) {
      set({ isRunningQuery: false })
      showErrorToast((error as Error).message || 'Unable to execute the SQL query.', 'SQL query failed')
      return null
    }
  },

  browseTable: async (table) => {
    const browse = getSavedBrowseState(get(), get().activeConnectionId, table.id)
    set({
      activeTableId: table.id,
      browse,
    })
    await get().loadTableRows(table.id)
  },

  updateTableRow: async (locator, values) => {
    const activeConnectionId = get().activeConnectionId
    const activeTableId = get().activeTableId
    if (!activeConnectionId || !activeTableId) return null

    set({ isMutatingRows: true })

    try {
      const result = await window.electronAPI.updateSqlRow({
        connectionId: activeConnectionId,
        tableId: activeTableId,
        locator,
        values,
      })

      set({ isMutatingRows: false })

      if (!result.success) {
        showErrorToast(result.error || 'Unable to update the selected row.', 'Row update failed')
        return result
      }

      showSuccessToast(result.message || 'Row updated successfully.', 'Row updated')
      await get().loadTableRows(activeTableId)
      return result
    } catch (error) {
      set({ isMutatingRows: false })
      showErrorToast((error as Error).message || 'Unable to update the selected row.', 'Row update failed')
      return null
    }
  },

  insertTableRow: async (values) => {
    const activeConnectionId = get().activeConnectionId
    const activeTableId = get().activeTableId
    if (!activeConnectionId || !activeTableId) return null

    set({ isMutatingRows: true })

    try {
      const result = await window.electronAPI.insertSqlRow({
        connectionId: activeConnectionId,
        tableId: activeTableId,
        values,
      })

      set({ isMutatingRows: false })

      if (!result.success) {
        showErrorToast(result.error || 'Unable to insert the new row.', 'Row insert failed')
        return result
      }

      showSuccessToast(result.message || 'Row inserted successfully.', 'Row inserted')
      await get().loadTableRows(activeTableId)
      return result
    } catch (error) {
      set({ isMutatingRows: false })
      showErrorToast((error as Error).message || 'Unable to insert the new row.', 'Row insert failed')
      return null
    }
  },

  deleteTableRow: async (locator) => {
    const activeConnectionId = get().activeConnectionId
    const activeTableId = get().activeTableId
    if (!activeConnectionId || !activeTableId) return null

    set({ isMutatingRows: true })

    try {
      const result = await window.electronAPI.deleteSqlRow({
        connectionId: activeConnectionId,
        tableId: activeTableId,
        locator,
      })

      set({ isMutatingRows: false })

      if (!result.success) {
        showErrorToast(result.error || 'Unable to delete the selected row.', 'Row delete failed')
        return result
      }

      showSuccessToast(result.message || 'Row deleted successfully.', 'Row deleted')
      await get().loadTableRows(activeTableId)
      return result
    } catch (error) {
      set({ isMutatingRows: false })
      showErrorToast((error as Error).message || 'Unable to delete the selected row.', 'Row delete failed')
      return null
    }
  },

  reset: () => set({
    databaseFiles: [],
    activeConnectionId: null,
    activeTableId: null,
    schema: null,
    queryResult: null,
    tableRows: null,
    resultMode: 'browser',
    queryText: DEFAULT_QUERY,
    queryHistory: readPersistedQueryHistory(),
    savedSnippets: readPersistedSavedSnippets(),
    browse: createDefaultBrowseState(),
    browseByTable: {},
    isLoadingFiles: false,
    isLoadingSchema: false,
    isLoadingTableRows: false,
    isRunningQuery: false,
    isMutatingRows: false,
  }),
}))
