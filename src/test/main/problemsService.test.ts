import path from 'path'
import { describe, expect, it } from 'vitest'
import { parseEslintProblems, parseTypeScriptProblems } from '../../main/services/problems'

describe('problemsService parsers', () => {
  it('parses TypeScript compiler output into normalized problem entries', () => {
    const rootPath = path.resolve('C:/workspace/sample')
    const output = [
      'src/app.ts(14,7): error TS2322: Type "number" is not assignable to type "string".',
      'src/util.ts(3,2): warning TS6133: "unused" is declared but its value is never read.',
      'This line should be ignored',
    ].join('\n')

    expect(parseTypeScriptProblems(output, rootPath)).toEqual([
      {
        id: `typescript:${path.resolve(rootPath, 'src/app.ts')}:14:7:TS2322`,
        filePath: path.resolve(rootPath, 'src/app.ts'),
        line: 14,
        column: 7,
        message: 'Type "number" is not assignable to type "string".',
        source: 'typescript',
        severity: 'error',
        code: 'TS2322',
      },
      {
        id: `typescript:${path.resolve(rootPath, 'src/util.ts')}:3:2:TS6133`,
        filePath: path.resolve(rootPath, 'src/util.ts'),
        line: 3,
        column: 2,
        message: '"unused" is declared but its value is never read.',
        source: 'typescript',
        severity: 'warning',
        code: 'TS6133',
      },
    ])
  })

  it('parses ESLint json output into normalized problem entries', () => {
    const rootPath = path.resolve('C:/workspace/sample')
    const output = JSON.stringify([
      {
        filePath: 'src/app.ts',
        messages: [
          {
            ruleId: 'no-console',
            severity: 1,
            message: 'Unexpected console statement.',
            line: 4,
            column: 3,
          },
          {
            ruleId: '@typescript-eslint/no-unused-vars',
            severity: 2,
            message: "'value' is assigned a value but never used.",
            line: 8,
            column: 11,
          },
          {
            ruleId: null,
            severity: 0,
            message: 'ignored',
          },
        ],
      },
    ])

    expect(parseEslintProblems(output, rootPath)).toEqual([
      {
        id: `eslint:${path.resolve(rootPath, 'src/app.ts')}:4:3:no-console`,
        filePath: path.resolve(rootPath, 'src/app.ts'),
        line: 4,
        column: 3,
        message: 'Unexpected console statement.',
        source: 'eslint',
        severity: 'warning',
        code: 'no-console',
      },
      {
        id: `eslint:${path.resolve(rootPath, 'src/app.ts')}:8:11:@typescript-eslint/no-unused-vars`,
        filePath: path.resolve(rootPath, 'src/app.ts'),
        line: 8,
        column: 11,
        message: "'value' is assigned a value but never used.",
        source: 'eslint',
        severity: 'error',
        code: '@typescript-eslint/no-unused-vars',
      },
    ])
  })
})
