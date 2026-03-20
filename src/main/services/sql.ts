import type {
  SqlBrowseTableRequest,
  SqlDeleteRowRequest,
  SqlDialect,
  SqlFeatureFlags,
  SqlInsertRowRequest,
  SqlMutationResult,
  SqlQueryRequest,
  SqlQueryResult,
  SqlSchemaSummary,
  SqlServiceStatus,
  SqlTableRowsResult,
  SqlUpdateRowRequest,
} from '@shared/types/sql'

export interface SqlAdapter {
  dialect: SqlDialect
  executeQuery: (request: SqlQueryRequest) => Promise<SqlQueryResult>
  getSchema: (connectionId: string) => Promise<SqlSchemaSummary>
  getTableRows: (request: SqlBrowseTableRequest) => Promise<SqlTableRowsResult>
  updateRow: (request: SqlUpdateRowRequest) => Promise<SqlMutationResult>
  insertRow: (request: SqlInsertRowRequest) => Promise<SqlMutationResult>
  deleteRow: (request: SqlDeleteRowRequest) => Promise<SqlMutationResult>
  dispose?: () => void
}

const supportedDialects: SqlDialect[] = ['sqlite', 'postgres', 'mysql']

const scaffoldFeatures: SqlFeatureFlags = {
  editor: true,
  queryRunner: false,
  schemaBrowser: false,
  rowEditing: false,
  savedQueries: false,
  explainPlan: false,
}

class SqlService {
  private readonly adapters = new Map<SqlDialect, SqlAdapter>()

  registerAdapter(adapter: SqlAdapter) {
    this.adapters.set(adapter.dialect, adapter)
  }

  getStatus(): SqlServiceStatus {
    return {
      phase: this.adapters.size > 0 ? 'active' : 'scaffold',
      supportedDialects,
      registeredDialects: [...this.adapters.keys()],
      connections: [],
      features: {
        ...scaffoldFeatures,
        queryRunner: this.adapters.size > 0,
        schemaBrowser: this.adapters.size > 0,
        rowEditing: this.adapters.size > 0,
      },
      message: this.adapters.size > 0
        ? 'SQL adapters are registered and ready for query execution.'
        : 'SQL service scaffolded. Add a database adapter to enable query execution and schema browsing.',
    }
  }

  async executeQuery(request: SqlQueryRequest): Promise<SqlQueryResult> {
    const adapter = this.adapters.get(this.resolveDialectFromConnectionId(request.connectionId))
    if (!adapter) {
      return {
        connectionId: request.connectionId,
        success: false,
        columns: [],
        rows: [],
        rowCount: 0,
        durationMs: 0,
        error: 'SQL query runner is not configured yet. The Phase 2 SQLite adapter will plug in here.',
        message: 'No SQL adapters are registered yet.',
      }
    }

    return adapter.executeQuery(request)
  }

  async getSchema(connectionId: string): Promise<SqlSchemaSummary> {
    const adapter = this.adapters.get(this.resolveDialectFromConnectionId(connectionId))
    if (!adapter) {
      return {
        connectionId,
        tables: [],
        columnsByTable: {},
        error: 'SQL schema browser is not configured yet. The Phase 3 data browser will plug in here.',
      }
    }

    return adapter.getSchema(connectionId)
  }

  async getTableRows(request: SqlBrowseTableRequest): Promise<SqlTableRowsResult> {
    const adapter = this.adapters.get(this.resolveDialectFromConnectionId(request.connectionId))
    if (!adapter) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        columns: [],
        rows: [],
        writable: false,
        totalRows: 0,
        error: 'SQL table browser is not configured yet.',
      }
    }

    return adapter.getTableRows(request)
  }

  async updateRow(request: SqlUpdateRowRequest): Promise<SqlMutationResult> {
    const adapter = this.adapters.get(this.resolveDialectFromConnectionId(request.connectionId))
    if (!adapter) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: false,
        rowCount: 0,
        error: 'SQL row editing is not configured yet.',
      }
    }

    return adapter.updateRow(request)
  }

  async insertRow(request: SqlInsertRowRequest): Promise<SqlMutationResult> {
    const adapter = this.adapters.get(this.resolveDialectFromConnectionId(request.connectionId))
    if (!adapter) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: false,
        rowCount: 0,
        error: 'SQL row inserts are not configured yet.',
      }
    }

    return adapter.insertRow(request)
  }

  async deleteRow(request: SqlDeleteRowRequest): Promise<SqlMutationResult> {
    const adapter = this.adapters.get(this.resolveDialectFromConnectionId(request.connectionId))
    if (!adapter) {
      return {
        connectionId: request.connectionId,
        tableId: request.tableId,
        success: false,
        rowCount: 0,
        error: 'SQL row deletes are not configured yet.',
      }
    }

    return adapter.deleteRow(request)
  }

  dispose() {
    this.adapters.forEach((adapter) => adapter.dispose?.())
  }

  private resolveDialectFromConnectionId(connectionId: string): SqlDialect {
    if (connectionId.startsWith('sqlite:')) return 'sqlite'
    if (connectionId.startsWith('postgres:')) return 'postgres'
    if (connectionId.startsWith('mysql:')) return 'mysql'
    return 'sqlite'
  }
}

export const sqlService = new SqlService()
