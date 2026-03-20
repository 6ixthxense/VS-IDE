import type { GitDiffRequest, RunCodePayload, SearchOptions, TaskDefinition, TaskRunRequest } from '../shared/types/ipc'
import type {
  SqlBrowseFilter,
  SqlBrowseTableRequest,
  SqlDeleteRowRequest,
  SqlFilterOperator,
  SqlInsertRowRequest,
  SqlQueryRequest,
  SqlRowLocator,
  SqlUpdateRowRequest,
} from '../shared/types/sql'

function requireString(value: unknown, label: string, options?: { trim?: boolean; allowEmpty?: boolean }) {
  if (typeof value !== 'string') {
    throw new TypeError(`${label} must be a string`)
  }

  const nextValue = options?.trim ? value.trim() : value
  if (!options?.allowEmpty && nextValue.length === 0) {
    throw new TypeError(`${label} must not be empty`)
  }

  return nextValue
}

function normalizePattern(value: unknown) {
  if (value == null) return undefined
  return requireString(value, 'Search pattern', { trim: true, allowEmpty: true })
}

export function requireCallback<T>(value: unknown, label: string): (payload: T) => void {
  if (typeof value !== 'function') {
    throw new TypeError(`${label} must be a function`)
  }

  return value as (payload: T) => void
}

export function requirePath(value: unknown, label = 'Path') {
  return requireString(value, label)
}

export function requireIdentifier(value: unknown, label: string) {
  return requireString(value, label, { trim: true })
}

export function requireMaybeEmptyString(value: unknown, label: string) {
  return requireString(value, label, { allowEmpty: true })
}

export function requireOptionalPath(value: unknown, label = 'Path') {
  if (value == null) return undefined
  return requirePath(value, label)
}

export function normalizeGitDiffRequest(
  request: Partial<GitDiffRequest> & { path?: unknown }
): GitDiffRequest {
  const record = requireRecord(request, 'Git diff request')

  if (record.staged != null && typeof record.staged !== 'boolean') {
    throw new TypeError('Git diff staged flag must be a boolean')
  }

  if (record.status != null && typeof record.status !== 'string') {
    throw new TypeError('Git diff status must be a string')
  }

  return {
    filePath: requirePath(record.filePath ?? record.path, 'Git diff path'),
    staged: record.staged as boolean | undefined,
    status: record.status as GitDiffRequest['status'],
  }
}

function requireStringArray(value: unknown, label: string) {
  if (!Array.isArray(value)) {
    throw new TypeError(`${label} must be an array`)
  }

  return value.map((entry, index) => requireString(entry, `${label} item ${index + 1}`, { allowEmpty: true }))
}

export function normalizeTaskDefinition(request: Partial<TaskDefinition>): TaskDefinition {
  const record = requireRecord(request, 'Task definition')
  const source = record.source

  if (source !== 'package-script' && source !== 'custom') {
    throw new TypeError('Task source must be either "package-script" or "custom"')
  }

  if (record.shell != null && typeof record.shell !== 'boolean') {
    throw new TypeError('Task shell flag must be a boolean')
  }

  return {
    id: requireIdentifier(record.id, 'Task id'),
    label: requireString(record.label, 'Task label', { trim: true }),
    command: requireString(record.command, 'Task command', { trim: true }),
    args: record.args == null ? [] : requireStringArray(record.args, 'Task args'),
    source,
    cwd: record.cwd == null ? undefined : requirePath(record.cwd, 'Task cwd'),
    detail: record.detail == null ? undefined : requireString(record.detail, 'Task detail', { allowEmpty: true }),
    shell: record.shell as boolean | undefined,
  }
}

export function normalizeTaskRunRequest(request: Partial<TaskRunRequest>): TaskRunRequest {
  const record = requireRecord(request, 'Task run request')

  return {
    terminalId: requireIdentifier(record.terminalId, 'Terminal id'),
    task: normalizeTaskDefinition(record.task as Partial<TaskDefinition>),
  }
}

export function normalizeSearchOptions(options?: Partial<SearchOptions>): Partial<SearchOptions> | undefined {
  if (!options) return undefined

  return {
    caseSensitive: Boolean(options.caseSensitive),
    wholeWord: Boolean(options.wholeWord),
    useRegex: Boolean(options.useRegex),
    includePattern: normalizePattern(options.includePattern),
    excludePattern: normalizePattern(options.excludePattern),
  }
}

export function normalizeRunCodePayload(payload: RunCodePayload): RunCodePayload {
  const terminalId = requireIdentifier(payload?.terminalId, 'Terminal id')
  const code = requireString(payload?.code, 'Code', { allowEmpty: true })
  const language = payload?.language

  if (language !== 'javascript' && language !== 'python') {
    throw new TypeError('Language must be either "javascript" or "python"')
  }

  return { terminalId, code, language }
}

function requirePositiveInteger(value: unknown, label: string) {
  if (!Number.isInteger(value) || Number(value) < 1) {
    throw new TypeError(`${label} must be a positive integer`)
  }

  return Number(value)
}

function requireNonNegativeInteger(value: unknown, label: string) {
  if (!Number.isInteger(value) || Number(value) < 0) {
    throw new TypeError(`${label} must be a non-negative integer`)
  }

  return Number(value)
}

function requireRecord(value: unknown, label: string) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError(`${label} must be an object`)
  }

  return value as Record<string, unknown>
}

function normalizeSqlValue(value: unknown): string | number | boolean | null {
  if (value == null) {
    return null
  }

  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return value
  }

  throw new TypeError('SQL values must be strings, numbers, booleans, or null')
}

function normalizeSqlValueRecord(value: unknown, label: string) {
  const record = requireRecord(value, label)

  return Object.fromEntries(
    Object.entries(record)
      .filter(([key]) => key.trim().length > 0)
      .map(([key, entryValue]) => [key, normalizeSqlValue(entryValue)])
  )
}

export function normalizeSqlRowLocator(locator: SqlRowLocator): SqlRowLocator {
  const kind = locator?.kind
  if (kind !== 'primaryKey' && kind !== 'rowid') {
    throw new TypeError('SQL row locator kind must be either "primaryKey" or "rowid"')
  }

  const values = normalizeSqlValueRecord(locator?.values, 'SQL row locator values')
  if (Object.keys(values).length === 0) {
    throw new TypeError('SQL row locator values must not be empty')
  }

  return { kind, values }
}

export function normalizeSqlQueryRequest(request: SqlQueryRequest): SqlQueryRequest {
  return {
    connectionId: requireIdentifier(request?.connectionId, 'SQL connection id'),
    sql: requireString(request?.sql, 'SQL query'),
    limit: request?.limit == null ? undefined : requirePositiveInteger(request.limit, 'SQL row limit'),
  }
}

const SQL_FILTER_OPERATORS = new Set<SqlFilterOperator>([
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

function normalizeSqlBrowseFilters(filters: unknown): SqlBrowseFilter[] | undefined {
  if (filters == null) return undefined
  if (!Array.isArray(filters)) {
    throw new TypeError('SQL filters must be an array')
  }

  return filters.map((filter, index) => {
    const record = requireRecord(filter, `SQL filter ${index + 1}`)
    const operator = record.operator
    if (!SQL_FILTER_OPERATORS.has(operator as SqlFilterOperator)) {
      throw new TypeError('SQL filter operator is not supported')
    }

    return {
      column: record.column == null ? undefined : requireIdentifier(record.column, 'SQL filter column'),
      operator: operator as SqlFilterOperator,
      value: record.value == null ? undefined : requireMaybeEmptyString(record.value, 'SQL filter value'),
    }
  })
}

export function normalizeSqlBrowseTableRequest(request: SqlBrowseTableRequest): SqlBrowseTableRequest {
  const sortDirection = request?.sortDirection
  if (sortDirection != null && sortDirection !== 'asc' && sortDirection !== 'desc') {
    throw new TypeError('SQL sort direction must be either "asc" or "desc"')
  }

  return {
    connectionId: requireIdentifier(request?.connectionId, 'SQL connection id'),
    tableId: requireIdentifier(request?.tableId, 'SQL table id'),
    limit: request?.limit == null ? undefined : requirePositiveInteger(request.limit, 'SQL row limit'),
    offset: request?.offset == null ? undefined : requireNonNegativeInteger(request.offset, 'SQL row offset'),
    sortColumn: request?.sortColumn == null ? undefined : requireIdentifier(request.sortColumn, 'SQL sort column'),
    sortDirection,
    filters: normalizeSqlBrowseFilters(request?.filters),
  }
}

export function normalizeSqlUpdateRowRequest(request: SqlUpdateRowRequest): SqlUpdateRowRequest {
  return {
    connectionId: requireIdentifier(request?.connectionId, 'SQL connection id'),
    tableId: requireIdentifier(request?.tableId, 'SQL table id'),
    locator: normalizeSqlRowLocator(request?.locator),
    values: normalizeSqlValueRecord(request?.values, 'SQL row values'),
  }
}

export function normalizeSqlInsertRowRequest(request: SqlInsertRowRequest): SqlInsertRowRequest {
  return {
    connectionId: requireIdentifier(request?.connectionId, 'SQL connection id'),
    tableId: requireIdentifier(request?.tableId, 'SQL table id'),
    values: normalizeSqlValueRecord(request?.values, 'SQL row values'),
  }
}

export function normalizeSqlDeleteRowRequest(request: SqlDeleteRowRequest): SqlDeleteRowRequest {
  return {
    connectionId: requireIdentifier(request?.connectionId, 'SQL connection id'),
    tableId: requireIdentifier(request?.tableId, 'SQL table id'),
    locator: normalizeSqlRowLocator(request?.locator),
  }
}
