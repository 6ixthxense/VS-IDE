import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, describe, expect, it } from 'vitest'
import { SqliteAdapter } from '../../main/services/sqliteAdapter'
import { workspaceService } from '../../main/services/workspace'

function createWorkspace() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-sqlite-'))
}

function canOpenBetterSqlite3InCurrentRuntime() {
  try {
    const BetterSqlite3 = require('better-sqlite3')
    const workspaceRoot = createWorkspace()
    const databasePath = path.join(workspaceRoot, 'probe.sqlite')
    const database = new BetterSqlite3(databasePath)
    database.close()
    return true
  } catch {
    return false
  }
}

describe('sqliteAdapter', () => {
  afterEach(() => {
    workspaceService.setRootPath(null)
  })

  const sqliteRuntimeCompatible = canOpenBetterSqlite3InCurrentRuntime()
  const runSqliteTest = sqliteRuntimeCompatible ? it : it.skip

  runSqliteTest('executes queries and introspects schema for workspace sqlite files', async () => {
    const workspaceRoot = createWorkspace()
    const databasePath = path.join(workspaceRoot, 'sample.sqlite')
    workspaceService.setRootPath(workspaceRoot)

    const adapter = new SqliteAdapter()
    const connectionId = `sqlite:${databasePath}`

    const createResult = await adapter.executeQuery({
      connectionId,
      sql: 'CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER, nickname TEXT);',
    })

    expect(createResult.success).toBe(true)

    const insertSeedResult = await adapter.executeQuery({
      connectionId,
      sql: `
        INSERT INTO users (name, age, nickname) VALUES
          ('Mickey', 32, NULL),
          ('Mini', 14, 'min'),
          ('Donald', 52, 'duck');
      `,
    })

    expect(insertSeedResult.success).toBe(true)

    const queryResult = await adapter.executeQuery({
      connectionId,
      sql: 'SELECT id, name FROM users ORDER BY id;',
    })

    expect(queryResult.success).toBe(true)
    expect(queryResult.columns).toEqual(['id', 'name'])
    expect(queryResult.rows).toEqual([
      { id: 1, name: 'Mickey' },
      { id: 2, name: 'Mini' },
      { id: 3, name: 'Donald' },
    ])

    const schema = await adapter.getSchema(connectionId)

    expect(schema.error).toBeUndefined()
    expect(schema.tables).toEqual([
      expect.objectContaining({ name: 'users', type: 'table' }),
    ])
    expect(schema.columnsByTable['main.users']).toEqual(expect.arrayContaining([
      expect.objectContaining({ name: 'id', isPrimaryKey: true }),
      expect.objectContaining({ name: 'name', dataType: 'TEXT' }),
      expect.objectContaining({ name: 'age', dataType: 'INTEGER' }),
      expect.objectContaining({ name: 'nickname', dataType: 'TEXT', nullable: true }),
    ]))

    const browserRows = await adapter.getTableRows({
      connectionId,
      tableId: 'main.users',
      limit: 10,
    })

    expect(browserRows.writable).toBe(true)
    expect(browserRows.rows[0]?.values).toEqual({ id: 1, name: 'Mickey', age: 32, nickname: null })

    const containsRows = await adapter.getTableRows({
      connectionId,
      tableId: 'main.users',
      limit: 10,
      filters: [
        {
          operator: 'contains',
          value: 'Duck',
        },
      ],
    })

    expect(containsRows.rows.map((row) => row.values.name)).toEqual(['Donald'])

    const startsWithRows = await adapter.getTableRows({
      connectionId,
      tableId: 'main.users',
      limit: 10,
      filters: [
        {
          column: 'name',
          operator: 'startsWith',
          value: 'Mi',
        },
      ],
    })

    expect(startsWithRows.rows.map((row) => row.values.name)).toEqual(['Mickey', 'Mini'])

    const numericRows = await adapter.getTableRows({
      connectionId,
      tableId: 'main.users',
      limit: 10,
      filters: [
        {
          column: 'age',
          operator: 'greaterThan',
          value: '20',
        },
      ],
    })

    expect(numericRows.rows.map((row) => row.values.name)).toEqual(['Mickey', 'Donald'])

    const nullRows = await adapter.getTableRows({
      connectionId,
      tableId: 'main.users',
      limit: 10,
      filters: [
        {
          column: 'nickname',
          operator: 'isNull',
        },
      ],
    })

    expect(nullRows.rows.map((row) => row.values.name)).toEqual(['Mickey'])

    const multiFilterRows = await adapter.getTableRows({
      connectionId,
      tableId: 'main.users',
      limit: 10,
      filters: [
        {
          column: 'age',
          operator: 'greaterThanOrEqual',
          value: '30',
        },
        {
          column: 'name',
          operator: 'contains',
          value: 'ck',
        },
      ],
    })

    expect(multiFilterRows.rows.map((row) => row.values.name)).toEqual(['Mickey'])

    const updateResult = await adapter.updateRow({
      connectionId,
      tableId: 'main.users',
      locator: { kind: 'primaryKey', values: { id: 1 } },
      values: { name: 'Mouse' },
    })

    expect(updateResult.success).toBe(true)

    const insertResult = await adapter.insertRow({
      connectionId,
      tableId: 'main.users',
      values: { name: 'Daisy', age: 28 },
    })

    expect(insertResult.success).toBe(true)

    const deleteResult = await adapter.deleteRow({
      connectionId,
      tableId: 'main.users',
      locator: { kind: 'primaryKey', values: { id: 4 } },
    })

    expect(deleteResult.success).toBe(true)

    const finalRows = await adapter.getTableRows({
      connectionId,
      tableId: 'main.users',
      limit: 10,
    })

    expect(finalRows.rows.map((row) => row.values)).toEqual([
      { id: 1, name: 'Mouse', age: 32, nickname: null },
      { id: 2, name: 'Mini', age: 14, nickname: 'min' },
      { id: 3, name: 'Donald', age: 52, nickname: 'duck' },
    ])

    adapter.dispose()
  })
})
