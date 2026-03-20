import { describe, expect, it } from 'vitest'
import {
  normalizeRunCodePayload,
  normalizeSearchOptions,
  normalizeSqlBrowseTableRequest,
  normalizeSqlDeleteRowRequest,
  normalizeSqlInsertRowRequest,
  normalizeSqlQueryRequest,
  normalizeSqlUpdateRowRequest,
  requireCallback,
  requireIdentifier,
  requirePath,
} from '../../preload/validation'

describe('preload validation helpers', () => {
  it('normalizes search flags and trims include/exclude patterns', () => {
    expect(normalizeSearchOptions({
      caseSensitive: 1 as unknown as boolean,
      wholeWord: 0 as unknown as boolean,
      useRegex: true,
      includePattern: ' src/**/*  ',
      excludePattern: ' dist/* ',
    })).toEqual({
      caseSensitive: true,
      wholeWord: false,
      useRegex: true,
      includePattern: 'src/**/*',
      excludePattern: 'dist/*',
    })
  })

  it('rejects invalid callbacks and blank identifiers', () => {
    expect(() => requireCallback(null, 'Listener')).toThrow('Listener must be a function')
    expect(() => requireIdentifier('   ', 'Terminal id')).toThrow('Terminal id must not be empty')
  })

  it('requires non-empty paths and valid run payloads', () => {
    expect(() => requirePath('', 'File path')).toThrow('File path must not be empty')

    expect(normalizeRunCodePayload({
      terminalId: '  default  ',
      code: 'print("ok")',
      language: 'python',
    })).toEqual({
      terminalId: 'default',
      code: 'print("ok")',
      language: 'python',
    })

    expect(() => normalizeRunCodePayload({
      terminalId: 'default',
      code: 'echo test',
      language: 'bash' as never,
    })).toThrow('Language must be either "javascript" or "python"')
  })

  it('normalizes SQL query requests', () => {
    expect(normalizeSqlQueryRequest({
      connectionId: ' sqlite:workspace ',
      sql: 'select 1',
      limit: 50,
    })).toEqual({
      connectionId: 'sqlite:workspace',
      sql: 'select 1',
      limit: 50,
    })

    expect(() => normalizeSqlQueryRequest({
      connectionId: 'sqlite:workspace',
      sql: '',
      limit: 0,
    })).toThrow()
  })

  it('normalizes SQL table browser and row mutation requests', () => {
    expect(normalizeSqlBrowseTableRequest({
      connectionId: ' sqlite:workspace ',
      tableId: ' main.users ',
      limit: 25,
      filters: [
        {
          column: 'age',
          operator: 'greaterThan',
          value: '18',
        },
        {
          operator: 'contains',
          value: 'mic',
        },
      ],
    })).toEqual({
      connectionId: 'sqlite:workspace',
      tableId: 'main.users',
      limit: 25,
      filters: [
        {
          column: 'age',
          operator: 'greaterThan',
          value: '18',
        },
        {
          operator: 'contains',
          value: 'mic',
        },
      ],
    })

    expect(normalizeSqlUpdateRowRequest({
      connectionId: 'sqlite:workspace',
      tableId: 'main.users',
      locator: {
        kind: 'rowid',
        values: { rowid: 1 },
      },
      values: { name: 'Mickey' },
    })).toEqual({
      connectionId: 'sqlite:workspace',
      tableId: 'main.users',
      locator: {
        kind: 'rowid',
        values: { rowid: 1 },
      },
      values: { name: 'Mickey' },
    })

    expect(normalizeSqlInsertRowRequest({
      connectionId: 'sqlite:workspace',
      tableId: 'main.users',
      values: { age: 12 },
    }).values).toEqual({ age: 12 })

    expect(normalizeSqlDeleteRowRequest({
      connectionId: 'sqlite:workspace',
      tableId: 'main.users',
      locator: {
        kind: 'primaryKey',
        values: { id: 1 },
      },
    }).locator.values).toEqual({ id: 1 })

    expect(() => normalizeSqlBrowseTableRequest({
      connectionId: 'sqlite:workspace',
      tableId: 'main.users',
      filters: [
        {
          column: 'age',
          operator: 'between' as never,
        },
      ],
    })).toThrow('SQL filter operator is not supported')
  })
})
