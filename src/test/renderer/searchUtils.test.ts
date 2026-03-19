import { describe, expect, it } from 'vitest'
import { buildReplacePreview, groupSearchResults, touchSearchHistory } from '../../renderer/features/search/utils'

describe('search utils', () => {
  it('groups search results by file path while preserving match order', () => {
    const groups = groupSearchResults([
      { path: 'C:/workspace/src/app.ts', name: 'app.ts', line: 3, match: 'hello world' },
      { path: 'C:/workspace/src/app.ts', name: 'app.ts', line: 8, match: 'hello again' },
      { path: 'C:/workspace/docs/readme.md', name: 'readme.md', line: 2, match: 'hello docs' },
    ], 'C:/workspace')

    expect(groups).toHaveLength(2)
    expect(groups[0]).toMatchObject({
      path: 'C:/workspace/src/app.ts',
      relativePath: 'src/app.ts',
    })
    expect(groups[0].matches.map((match) => match.line)).toEqual([3, 8])
    expect(groups[1].relativePath).toBe('docs/readme.md')
  })

  it('builds a replace preview from the active search options', () => {
    const preview = buildReplacePreview('const helloWorld = "hello";', 'hello', 'updated', {
      caseSensitive: false,
      wholeWord: false,
      useRegex: false,
    })

    expect(preview).toBe('const updatedWorld = "updated";')
  })

  it('deduplicates search history while keeping the most recent items first', () => {
    const history = touchSearchHistory(['status', 'hello', 'replace'], 'hello')

    expect(history.slice(0, 3)).toEqual(['hello', 'status', 'replace'])
  })
})
