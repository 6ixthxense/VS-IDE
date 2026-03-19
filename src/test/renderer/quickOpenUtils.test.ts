import { describe, expect, it } from 'vitest'
import { buildQuickOpenItems, findFuzzyMatch, parseQuickOpenQuery } from '../../renderer/features/quick-open/utils'

describe('quickOpen utils', () => {
  const rootPath = 'C:\\workspace'
  const files = [
    'C:\\workspace\\src\\AppShell.tsx',
    'C:\\workspace\\src\\api\\accountService.ts',
    'C:\\workspace\\docs\\architecture.md',
  ]

  it('parses optional line-jump suffixes from the query', () => {
    expect(parseQuickOpenQuery('AppShell:120')).toEqual({ fileQuery: 'AppShell', lineNumber: 120 })
    expect(parseQuickOpenQuery(':42')).toEqual({ fileQuery: '', lineNumber: 42 })
    expect(parseQuickOpenQuery('service')).toEqual({ fileQuery: 'service' })
  })

  it('finds fuzzy subsequence matches and tracks their indexes', () => {
    expect(findFuzzyMatch('AppShell.tsx', 'ash')).toMatchObject({
      indexes: [0, 3, 4],
    })
    expect(findFuzzyMatch('AppShell.tsx', 'xyz')).toBeNull()
  })

  it('boosts recent files and filename matches when building results', () => {
    const { items } = buildQuickOpenItems(files, rootPath, 'acc', ['C:\\workspace\\src\\api\\accountService.ts'])

    expect(items[0]).toMatchObject({
      fileName: 'accountService.ts',
      isRecent: true,
    })
  })

  it('shows recent files first when the query is empty', () => {
    const { items } = buildQuickOpenItems(files, rootPath, '', [
      'C:\\workspace\\docs\\architecture.md',
      'C:\\workspace\\src\\AppShell.tsx',
    ])

    expect(items.slice(0, 2).map((item) => item.fileName)).toEqual(['architecture.md', 'AppShell.tsx'])
  })
})
