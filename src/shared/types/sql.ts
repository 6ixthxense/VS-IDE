export type SqlDialect = 'sqlite' | 'postgres' | 'mysql'

export interface SqlConnectionProfile {
  id: string
  name: string
  dialect: SqlDialect
  status: 'disconnected' | 'ready' | 'error'
  database?: string
  host?: string
  readOnly?: boolean
}

export interface SqlFeatureFlags {
  editor: boolean
  queryRunner: boolean
  schemaBrowser: boolean
  rowEditing: boolean
  savedQueries: boolean
  explainPlan: boolean
}

export interface SqlServiceStatus {
  phase: 'scaffold' | 'active'
  supportedDialects: SqlDialect[]
  registeredDialects: SqlDialect[]
  connections: SqlConnectionProfile[]
  features: SqlFeatureFlags
  message: string
}

export interface SqlColumnMetadata {
  name: string
  dataType: string
  nullable: boolean
  isPrimaryKey: boolean
}

export interface SqlTableSummary {
  id: string
  schema: string | null
  name: string
  type: 'table' | 'view'
}

export interface SqlSchemaSummary {
  connectionId: string
  tables: SqlTableSummary[]
  columnsByTable: Record<string, SqlColumnMetadata[]>
  error?: string
}

export interface SqlQueryRequest {
  connectionId: string
  sql: string
  limit?: number
}

export interface SqlQueryResult {
  connectionId: string
  success: boolean
  columns: string[]
  rows: Array<Record<string, unknown>>
  rowCount: number
  durationMs: number
  error?: string
  message?: string
}

export interface SqlRowLocator {
  kind: 'primaryKey' | 'rowid'
  values: Record<string, unknown>
}

export interface SqlTableRow {
  locator: SqlRowLocator | null
  values: Record<string, unknown>
}

export type SqlFilterOperator =
  | 'contains'
  | 'startsWith'
  | 'endsWith'
  | 'equals'
  | 'notEquals'
  | 'greaterThan'
  | 'greaterThanOrEqual'
  | 'lessThan'
  | 'lessThanOrEqual'
  | 'isNull'
  | 'isNotNull'

export interface SqlBrowseFilter {
  column?: string
  operator: SqlFilterOperator
  value?: string
}

export interface SqlBrowseTableRequest {
  connectionId: string
  tableId: string
  limit?: number
  offset?: number
  sortColumn?: string
  sortDirection?: 'asc' | 'desc'
  filters?: SqlBrowseFilter[]
}

export interface SqlTableRowsResult {
  connectionId: string
  tableId: string
  columns: SqlColumnMetadata[]
  rows: SqlTableRow[]
  writable: boolean
  limit?: number
  offset?: number
  totalRows: number
  sortColumn?: string
  sortDirection?: 'asc' | 'desc'
  filters?: SqlBrowseFilter[]
  error?: string
  message?: string
}

export interface SqlUpdateRowRequest {
  connectionId: string
  tableId: string
  locator: SqlRowLocator
  values: Record<string, unknown>
}

export interface SqlInsertRowRequest {
  connectionId: string
  tableId: string
  values: Record<string, unknown>
}

export interface SqlDeleteRowRequest {
  connectionId: string
  tableId: string
  locator: SqlRowLocator
}

export interface SqlMutationResult {
  connectionId: string
  tableId: string
  success: boolean
  rowCount: number
  error?: string
  message?: string
}
