import type BetterSqlite3 from 'better-sqlite3'
import type { SqlAdapter } from './sql'
import type {
  SqlBrowseFilter,
  SqlBrowseTableRequest,
  SqlColumnMetadata,
  SqlDeleteRowRequest,
  SqlFilterOperator,
  SqlInsertRowRequest,
  SqlMutationResult,
  SqlQueryRequest,
  SqlQueryResult,
  SqlRowLocator,
  SqlSchemaSummary,
  SqlTableRow,
  SqlTableRowsResult,
  SqlTableSummary,
  SqlUpdateRowRequest,
} from '@shared/types/sql'
import { workspaceService } from './workspace'

type SqliteDatabaseHandle = InstanceType<typeof BetterSqlite3>
type SqliteDriver = typeof BetterSqlite3

interface SqliteColumnInfo {
  name: string
  type: string
  notnull: number
  pk: number
}

interface SqliteBrowseQueryParts {
  whereClause: string
  params: Record<string, unknown>
  orderClause: string
}

const SQLITE_TEXT_FILTER_OPERATORS = new Set<SqlFilterOperator>(['contains', 'startsWith', 'endsWith'])
const SQLITE_NULL_FILTER_OPERATORS = new Set<SqlFilterOperator>(['isNull', 'isNotNull'])
let sqliteDriver: SqliteDriver | null = null

function quoteIdentifier(value: string) {
  return `"${value.replace(/"/g, '""')}"`
}

export function normalizeSqliteDriverError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error)

  if (
    message.includes('compiled against a different Node.js version') ||
    message.includes('NODE_MODULE_VERSION')
  ) {
    return 'The SQLite native module does not match this Electron runtime. Run "npm run rebuild:native" in the project root, then restart the app.'
  }

  return message
}

function loadSqliteDriver(): SqliteDriver {
  if (sqliteDriver) {
    return sqliteDriver
  }

  try {
    sqliteDriver = require('better-sqlite3') as SqliteDriver
    return sqliteDriver
  } catch (error) {
    throw new Error(normalizeSqliteDriverError(error))
  }
}

function normalizeCellValue(value: unknown): unknown {
  if (value == null || typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return value
  }

  if (typeof value === 'bigint') {
    return value.toString()
  }

  if (value instanceof Date) {
    return value.toISOString()
  }

  if (Buffer.isBuffer(value)) {
    const hex = value.toString('hex')
    return hex.length <= 48 ? `0x${hex}` : `0x${hex.slice(0, 48)}...`
  }

  if (value instanceof Uint8Array) {
    const hex = Buffer.from(value).toString('hex')
    return hex.length <= 48 ? `0x${hex}` : `0x${hex.slice(0, 48)}...`
  }

  if (Array.isArray(value) || typeof value === 'object') {
    try {
      return JSON.stringify(value)
    } catch {
      return String(value)
    }
  }

  return String(value)
}

function normalizeRow(row: Record<string, unknown>) {
  return Object.fromEntries(
    Object.entries(row).map(([key, value]) => [key, normalizeCellValue(value)])
  )
}

function buildTableId(schema: string | null, name: string) {
  return `${schema ?? 'main'}.${name}`
}

function isSqliteScript(sql: string) {
  const cleaned = sql.trim().replace(/;+$/, '')
  return cleaned.includes(';')
}

function mapColumnMetadata(columns: SqliteColumnInfo[]): SqlColumnMetadata[] {
  return columns
    .sort((left, right) => (left.pk || Number.MAX_SAFE_INTEGER) - (right.pk || Number.MAX_SAFE_INTEGER))
    .map((column) => ({
      name: column.name,
      dataType: column.type || 'TEXT',
      nullable: column.notnull === 0,
      isPrimaryKey: column.pk > 0,
    }))
}

function parseTableId(tableId: string) {
  const separatorIndex = tableId.indexOf('.')
  return separatorIndex === -1
    ? { schema: 'main', tableName: tableId }
    : { schema: tableId.slice(0, separatorIndex), tableName: tableId.slice(separatorIndex + 1) }
}

function buildWhereClause(locator: SqlRowLocator) {
  if (locator.kind === 'rowid') {
    if (!Object.prototype.hasOwnProperty.call(locator.values, 'rowid')) {
      throw new Error('Row locator is missing the SQLite rowid value.')
    }

    return {
      clause: 'rowid = @locator_rowid',
      params: { locator_rowid: locator.values.rowid },
    }
  }

  const entries = Object.entries(locator.values)
  if (entries.length === 0) {
    throw new Error('Primary key locator must include at least one key column.')
  }

  return {
    clause: entries.map(([key]) => `${quoteIdentifier(key)} = @locator_${key}`).join(' AND '),
    params: Object.fromEntries(entries.map(([key, value]) => [`locator_${key}`, value])),
  }
}

function escapeLikePattern(value: string) {
  return value.replace(/[\\%_]/g, '\\$&')
}

function parseFilterValue(rawValue: string, column: SqlColumnMetadata) {
  const trimmed = rawValue.trim()
  if (trimmed.length === 0) {
    return rawValue
  }

  const dataType = column.dataType.toUpperCase()

  if (dataType.includes('BOOL')) {
    if (['true', '1'].includes(trimmed.toLowerCase())) return 1
    if (['false', '0'].includes(trimmed.toLowerCase())) return 0
  }

  if (dataType.includes('INT') && /^-?\d+$/.test(trimmed)) {
    return Number.parseInt(trimmed, 10)
  }

  if (
    (dataType.includes('REAL') || dataType.includes('FLOA') || dataType.includes('DOUB') || dataType.includes('NUM') || dataType.includes('DEC')) &&
    !Number.isNaN(Number(trimmed))
  ) {
    return Number(trimmed)
  }

  return rawValue
}

function buildColumnFilterClause(
  column: SqlColumnMetadata,
  operator: SqlFilterOperator,
  paramName: string,
  rawValue: string | undefined
) {
  const columnExpression = quoteIdentifier(column.name)

  if (operator === 'isNull') {
    return {
      clause: `${columnExpression} IS NULL`,
      params: {},
    }
  }

  if (operator === 'isNotNull') {
    return {
      clause: `${columnExpression} IS NOT NULL`,
      params: {},
    }
  }

  if (!rawValue || rawValue.trim().length === 0) {
    return null
  }

  if (SQLITE_TEXT_FILTER_OPERATORS.has(operator)) {
    const escapedValue = escapeLikePattern(rawValue.trim())
    const pattern = operator === 'startsWith'
      ? `${escapedValue}%`
      : operator === 'endsWith'
        ? `%${escapedValue}`
        : `%${escapedValue}%`

    return {
      clause: `CAST(${columnExpression} AS TEXT) LIKE @${paramName} ESCAPE '\\'`,
      params: { [paramName]: pattern },
    }
  }

  let comparisonOperator: '=' | '!=' | '>' | '>=' | '<' | '<=' | null = null
  switch (operator) {
    case 'equals':
      comparisonOperator = '='
      break
    case 'notEquals':
      comparisonOperator = '!='
      break
    case 'greaterThan':
      comparisonOperator = '>'
      break
    case 'greaterThanOrEqual':
      comparisonOperator = '>='
      break
    case 'lessThan':
      comparisonOperator = '<'
      break
    case 'lessThanOrEqual':
      comparisonOperator = '<='
      break
    default:
      comparisonOperator = null
      break
  }

  if (!comparisonOperator) {
    throw new Error('The selected filter operator is not supported by SQLite browsing.')
  }

  return {
    clause: `${columnExpression} ${comparisonOperator} @${paramName}`,
    params: { [paramName]: parseFilterValue(rawValue, column) },
  }
}

function buildFilterClause(
  filter: SqlBrowseFilter,
  index: number,
  columns: SqlColumnMetadata[],
  columnsByName: Map<string, SqlColumnMetadata>
) {
  const operator = filter.operator
  const baseParamName = `filter_${index}`

  if (SQLITE_NULL_FILTER_OPERATORS.has(operator)) {
    if (!filter.column) {
      throw new Error('Choose a column before using null filters.')
    }

    const filterColumn = columnsByName.get(filter.column)
    if (!filterColumn) {
      throw new Error('The selected filter column is no longer available.')
    }

    return buildColumnFilterClause(filterColumn, operator, baseParamName, filter.value)
  }

  if (!filter.value || filter.value.trim().length === 0) {
    return null
  }

  if (filter.column) {
    const filterColumn = columnsByName.get(filter.column)
    if (!filterColumn) {
      throw new Error('The selected filter column is no longer available.')
    }

    return buildColumnFilterClause(filterColumn, operator, baseParamName, filter.value)
  }

  if (operator !== 'contains') {
    throw new Error('Choose a column before using this filter operator.')
  }

  const predicates = columns.flatMap((column, columnIndex) => {
    const filterClause = buildColumnFilterClause(column, operator, `${baseParamName}_${columnIndex}`, filter.value)
    if (!filterClause) {
      return []
    }

    return [filterClause]
  })

  if (predicates.length === 0) {
    return null
  }

  return {
    clause: `(${predicates.map((predicate) => predicate.clause).join(' OR ')})`,
    params: Object.assign({}, ...predicates.map((predicate) => predicate.params)),
  }
}

function buildBrowseQueryParts(
  request: SqlBrowseTableRequest,
  columns: SqlColumnMetadata[]
): SqliteBrowseQueryParts {
  const columnNames = new Set(columns.map((column) => column.name))
  const columnsByName = new Map(columns.map((column) => [column.name, column]))
  const params: Record<string, unknown> = {}
  let whereClause = ''
  const filterClauses = (request.filters ?? [])
    .flatMap((filter, index) => {
      const filterClause = buildFilterClause(filter, index, columns, columnsByName)
      if (!filterClause) {
        return []
      }

      Object.assign(params, filterClause.params)
      return [filterClause.clause]
    })

  if (filterClauses.length > 0) {
    whereClause = `WHERE ${filterClauses.join(' AND ')}`
  }

  let orderClause = ''
  if (request.sortColumn) {
    if (!columnNames.has(request.sortColumn)) {
      throw new Error('The selected sort column is no longer available.')
    }

    orderClause = `ORDER BY ${quoteIdentifier(request.sortColumn)} ${request.sortDirection === 'desc' ? 'DESC' : 'ASC'}`
  }

  return { whereClause, params, orderClause }
}

export class SqliteAdapter implements SqlAdapter {
  readonly dialect = 'sqlite' as const
  private readonly databases = new Map<string, SqliteDatabaseHandle>()

  async executeQuery(request: SqlQueryRequest): Promise<SqlQueryResult> {
    const sql = request.sql.trim()
    const startedAt = Date.now()

    if (!sql) {
      return {
        connectionId: request.connectionId,
        success: false,
        columns: [],
        rows: [],
        rowCount: 0,
        durationMs: 0,
        error: 'Enter a SQL statement before running the console.',
      }
    }

    try {
      const database = this.openDatabase(request.connectionId)

      if (isSqliteScript(sql)) {
        database.exec(sql)
        return {
          connectionId: request.connectionId,
          success: true,
          columns: [],
          rows: [],
          rowCount: 0,
          durationMs: Date.now() - startedAt,
          message: 'Executed SQL script successfully.',
        }
      }

      const statement = database.prepare(sql)

      if (statement.reader) {
        const rows = (statement.all() as Array<Record<string, unknown>>).map(normalizeRow)

        return {
          connectionId: request.connectionId,
          success: true,
          columns: Object.keys(rows[0] ?? {}),
          rows,
          rowCount: rows.length,
          durationMs: Date.now() - startedAt,
          message: rows.length === 1 ? 'Returned 1 row.' : `Returned ${rows.length} rows.`,
        }
      }

      const info = statement.run()
      return {
        connectionId: request.connectionId,
        success: true,
        columns: [],
        rows: [],
        rowCount: info.changes,
        durationMs: Date.now() - startedAt,
        message: info.changes === 1 ? '1 row affected.' : `${info.changes} rows affected.`,
      }
    } catch (error) {
      return {
        connectionId: request.connectionId,
        success: false,
        columns: [],
        rows: [],
        rowCount: 0,
        durationMs: Date.now() - startedAt,
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  async getSchema(connectionId: string): Promise<SqlSchemaSummary> {
    try {
      const database = this.openDatabase(connectionId)
      const tables = database.prepare(`
        SELECT name, type
        FROM sqlite_master
        WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%'
        ORDER BY type, name
      `).all() as Array<{ name: string; type: 'table' | 'view' }>

      const tableSummaries: SqlTableSummary[] = tables.map((table) => ({
        id: buildTableId('main', table.name),
        schema: 'main',
        name: table.name,
        type: table.type,
      }))

      const columnsByTable = Object.fromEntries(
        tableSummaries.map((table) => [
          table.id,
          this.describeTable(database, table.id).columns,
        ])
      )

      return {
        connectionId,
        tables: tableSummaries,
        columnsByTable,
      }
    } catch (error) {
      return {
        connectionId,
        tables: [],
        columnsByTable: {},
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  async getTableRows(request: SqlBrowseTableRequest): Promise<SqlTableRowsResult> {
    try {
      const database = this.openDatabase(request.connectionId)
      const table = this.describeTable(database, request.tableId)
      const limit = request.limit ?? 100
      const offset = request.offset ?? 0
      const browseQuery = buildBrowseQueryParts(request, table.columns)
      const countRow = database.prepare(`
        SELECT COUNT(*) AS totalRows
        FROM ${quoteIdentifier(table.tableName)}
        ${browseQuery.whereClause}
      `).get(browseQuery.params) as { totalRows: number }
      const totalRows = countRow?.totalRows ?? 0

      if (table.type === 'view') {
        const rows = (database.prepare(`
          SELECT *
          FROM ${quoteIdentifier(table.tableName)}
          ${browseQuery.whereClause}
          ${browseQuery.orderClause}
          LIMIT @limit OFFSET @offset
        `).all({ ...browseQuery.params, limit, offset }) as Array<Record<string, unknown>>)
          .map<SqlTableRow>((row) => ({
            locator: null,
            values: normalizeRow(row),
          }))

        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          columns: table.columns,
          rows,
          writable: false,
          limit,
          offset,
          totalRows,
          sortColumn: request.sortColumn,
          sortDirection: request.sortDirection,
          filters: request.filters,
          message: 'Views are shown read-only.',
        }
      }

      if (table.primaryKeyColumns.length > 0) {
        const rows = (database.prepare(`
          SELECT *
          FROM ${quoteIdentifier(table.tableName)}
          ${browseQuery.whereClause}
          ${browseQuery.orderClause}
          LIMIT @limit OFFSET @offset
        `).all({ ...browseQuery.params, limit, offset }) as Array<Record<string, unknown>>)
          .map<SqlTableRow>((row) => ({
            locator: {
              kind: 'primaryKey',
              values: Object.fromEntries(table.primaryKeyColumns.map((columnName) => [columnName, row[columnName]])),
            },
            values: normalizeRow(row),
          }))

        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          columns: table.columns,
          rows,
          writable: true,
          limit,
          offset,
          totalRows,
          sortColumn: request.sortColumn,
          sortDirection: request.sortDirection,
          filters: request.filters,
          message: rows.length === 1 ? 'Loaded 1 row.' : `Loaded ${rows.length} rows.`,
        }
      }

      try {
        const rows = (database.prepare(`
          SELECT rowid AS __sql_rowid, *
          FROM ${quoteIdentifier(table.tableName)}
          ${browseQuery.whereClause}
          ${browseQuery.orderClause}
          LIMIT @limit OFFSET @offset
        `).all({ ...browseQuery.params, limit, offset }) as Array<Record<string, unknown>>)
          .map<SqlTableRow>((row) => {
            const { __sql_rowid, ...rest } = row
            return {
              locator: { kind: 'rowid', values: { rowid: __sql_rowid } },
              values: normalizeRow(rest),
            }
          })

        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          columns: table.columns,
          rows,
          writable: true,
          limit,
          offset,
          totalRows,
          sortColumn: request.sortColumn,
          sortDirection: request.sortDirection,
          filters: request.filters,
          message: rows.length === 1 ? 'Loaded 1 row.' : `Loaded ${rows.length} rows.`,
        }
      } catch {
        const rows = (database.prepare(`
          SELECT *
          FROM ${quoteIdentifier(table.tableName)}
          ${browseQuery.whereClause}
          ${browseQuery.orderClause}
          LIMIT @limit OFFSET @offset
        `).all({ ...browseQuery.params, limit, offset }) as Array<Record<string, unknown>>)
          .map<SqlTableRow>((row) => ({
            locator: null,
            values: normalizeRow(row),
          }))

        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          columns: table.columns,
          rows,
          writable: false,
          limit,
          offset,
          totalRows,
          sortColumn: request.sortColumn,
          sortDirection: request.sortDirection,
          filters: request.filters,
          message: 'Loaded rows in read-only mode because the table has no primary key or rowid access.',
        }
      }
    } catch (error) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        columns: [],
        rows: [],
        writable: false,
        totalRows: 0,
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  async updateRow(request: SqlUpdateRowRequest): Promise<SqlMutationResult> {
    try {
      const database = this.openDatabase(request.connectionId)
      const table = this.describeTable(database, request.tableId)

      if (table.type === 'view') {
        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          success: false,
          rowCount: 0,
          error: 'Views are read-only.',
        }
      }

      const validColumnNames = new Set(table.columns.map((column) => column.name))
      const nextValues = Object.entries(request.values)
        .filter(([columnName]) => validColumnNames.has(columnName))

      if (nextValues.length === 0) {
        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          success: false,
          rowCount: 0,
          error: 'Choose at least one editable column before saving the row.',
        }
      }

      const setClause = nextValues.map(([columnName], index) => `${quoteIdentifier(columnName)} = @value_${index}`).join(', ')
      const valueParams = Object.fromEntries(nextValues.map(([, value], index) => [`value_${index}`, value]))
      const where = buildWhereClause(request.locator)

      const info = database.prepare(`
        UPDATE ${quoteIdentifier(table.tableName)}
        SET ${setClause}
        WHERE ${where.clause}
      `).run({ ...valueParams, ...where.params })

      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: true,
        rowCount: info.changes,
        message: info.changes === 1 ? 'Row updated.' : `${info.changes} rows updated.`,
      }
    } catch (error) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: false,
        rowCount: 0,
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  async insertRow(request: SqlInsertRowRequest): Promise<SqlMutationResult> {
    try {
      const database = this.openDatabase(request.connectionId)
      const table = this.describeTable(database, request.tableId)

      if (table.type === 'view') {
        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          success: false,
          rowCount: 0,
          error: 'Views are read-only.',
        }
      }

      const validColumnNames = new Set(table.columns.map((column) => column.name))
      const nextValues = Object.entries(request.values)
        .filter(([columnName]) => validColumnNames.has(columnName))

      const info = nextValues.length === 0
        ? database.prepare(`INSERT INTO ${quoteIdentifier(table.tableName)} DEFAULT VALUES`).run()
        : database.prepare(`
            INSERT INTO ${quoteIdentifier(table.tableName)} (${nextValues.map(([columnName]) => quoteIdentifier(columnName)).join(', ')})
            VALUES (${nextValues.map((_, index) => `@value_${index}`).join(', ')})
          `).run(Object.fromEntries(nextValues.map(([, value], index) => [`value_${index}`, value])))

      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: true,
        rowCount: info.changes,
        message: info.changes === 1 ? 'Row inserted.' : `${info.changes} rows inserted.`,
      }
    } catch (error) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: false,
        rowCount: 0,
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  async deleteRow(request: SqlDeleteRowRequest): Promise<SqlMutationResult> {
    try {
      const database = this.openDatabase(request.connectionId)
      const table = this.describeTable(database, request.tableId)

      if (table.type === 'view') {
        return {
          connectionId: request.connectionId,
          tableId: request.tableId,
          success: false,
          rowCount: 0,
          error: 'Views are read-only.',
        }
      }

      const where = buildWhereClause(request.locator)
      const info = database.prepare(`
        DELETE FROM ${quoteIdentifier(table.tableName)}
        WHERE ${where.clause}
      `).run(where.params)

      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: true,
        rowCount: info.changes,
        message: info.changes === 1 ? 'Row deleted.' : `${info.changes} rows deleted.`,
      }
    } catch (error) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: false,
        rowCount: 0,
        error: error instanceof Error ? error.message : String(error),
      }
    }
  }

  dispose() {
    this.databases.forEach((database) => database.close())
    this.databases.clear()
  }

  private describeTable(database: SqliteDatabaseHandle, tableId: string) {
    const { tableName } = parseTableId(tableId)
    const tableMeta = database.prepare(`
      SELECT name, type
      FROM sqlite_master
      WHERE name = ? AND type IN ('table', 'view')
      LIMIT 1
    `).get(tableName) as { name: string; type: 'table' | 'view' } | undefined

    if (!tableMeta) {
      throw new Error('The selected table could not be found in this SQLite file.')
    }

    const rawColumns = database.prepare(`PRAGMA table_info(${quoteIdentifier(tableName)})`).all() as SqliteColumnInfo[]

    if (rawColumns.length === 0) {
      throw new Error('The selected table does not expose any columns.')
    }

    const primaryKeyColumns = rawColumns
      .filter((column) => column.pk > 0)
      .sort((left, right) => left.pk - right.pk)
      .map((column) => column.name)

    return {
      tableName,
      type: tableMeta.type,
      columns: mapColumnMetadata(rawColumns),
      primaryKeyColumns,
    }
  }

  private openDatabase(connectionId: string) {
    const databasePath = this.resolveDatabasePath(connectionId)
    const cached = this.databases.get(databasePath)
    if (cached) {
      return cached
    }

    const BetterSqlite3 = loadSqliteDriver()
    const database = new BetterSqlite3(databasePath)
    this.databases.set(databasePath, database)
    return database
  }

  private resolveDatabasePath(connectionId: string) {
    const rawPath = connectionId.startsWith('sqlite:') ? connectionId.slice('sqlite:'.length) : connectionId
    return workspaceService.assertWithinWorkspace(rawPath, 'SQLite database path')
  }
}
