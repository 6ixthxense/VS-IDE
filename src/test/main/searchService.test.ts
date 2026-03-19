import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { replaceInWorkspace, searchWorkspace, shouldSearchPath } from '../../main/services/search'
import { workspaceFileIndex } from '../../main/services/fileIndex'

describe('searchService', () => {
  let rootPath = ''

  beforeEach(() => {
    rootPath = fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-ide-search-'))
    fs.mkdirSync(path.join(rootPath, 'src'))
    fs.mkdirSync(path.join(rootPath, 'docs'))
    fs.mkdirSync(path.join(rootPath, 'dist'))

    fs.writeFileSync(
      path.join(rootPath, 'src', 'app.tsx'),
      [
        'const HelloWorld = "Hello";',
        'const helloWorld = "hello";',
        'console.log(helloWorld);',
      ].join('\n'),
      'utf-8'
    )
    fs.writeFileSync(path.join(rootPath, 'docs', 'readme.md'), 'hello from docs\n', 'utf-8')
    fs.writeFileSync(path.join(rootPath, 'dist', 'bundle.js'), 'hello from dist\n', 'utf-8')
  })

  afterEach(() => {
    workspaceFileIndex.setRootPath(null)
    fs.rmSync(rootPath, { recursive: true, force: true })
  })

  it('filters search results with include/exclude patterns and whole word matching', async () => {
    const results = await searchWorkspace(rootPath, 'hello', {
      wholeWord: true,
      caseSensitive: false,
      includePattern: 'src/*',
      excludePattern: '*.md',
    })

    expect(results).toHaveLength(2)
    expect(results.every((result) => result.path.endsWith(path.join('src', 'app.tsx')))).toBe(true)
  })

  it('replaces matches using regex filters while skipping excluded paths', async () => {
    const result = await replaceInWorkspace(rootPath, 'hello(world)?', 'updated', {
      useRegex: true,
      caseSensitive: false,
      excludePattern: 'dist/*, docs/*',
    })

    expect(result).toMatchObject({ success: true, count: 1 })
    expect(fs.readFileSync(path.join(rootPath, 'src', 'app.tsx'), 'utf-8')).toContain('updated')
    expect(fs.readFileSync(path.join(rootPath, 'docs', 'readme.md'), 'utf-8')).toContain('hello from docs')
  })

  it('evaluates include and exclude patterns against normalized relative paths', () => {
    expect(shouldSearchPath('src/app.tsx', { includePattern: 'src/*' })).toBe(true)
    expect(shouldSearchPath('docs/readme.md', { includePattern: 'src/*' })).toBe(false)
    expect(shouldSearchPath('dist/bundle.js', { excludePattern: 'dist/*' })).toBe(false)
  })
})
