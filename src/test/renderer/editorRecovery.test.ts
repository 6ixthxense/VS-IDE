import { describe, expect, it } from 'vitest'
import type { Tab } from '@shared/types/file'
import { resolveRestoredTab, syncTabWithDisk } from '../../renderer/utils/editorRecovery'

function createTab(overrides: Partial<Tab> = {}): Tab {
  return {
    id: 'app.ts',
    path: 'C:\\workspace\\app.ts',
    name: 'app.ts',
    language: 'typescript',
    content: 'const value = 2\n',
    savedContent: 'const value = 1\n',
    ...overrides,
  }
}

describe('editorRecovery', () => {
  it('marks unsaved session content as recovered when disk content is unchanged', () => {
    const resolution = resolveRestoredTab({
      id: 'app.ts',
      path: 'C:\\workspace\\app.ts',
      name: 'app.ts',
      language: 'typescript',
      content: 'const value = 2\n',
      savedContent: 'const value = 1\n',
    }, 'const value = 1\n')

    expect(resolution.issue).toBe('recovered')
    expect(resolution.tab.content).toBe('const value = 2\n')
    expect(resolution.tab.savedContent).toBe('const value = 1\n')
    expect(resolution.tab.recoveryState).toBe('recovered')
  })

  it('marks restored tabs as conflicted when disk content changed separately', () => {
    const resolution = resolveRestoredTab({
      id: 'app.ts',
      path: 'C:\\workspace\\app.ts',
      name: 'app.ts',
      language: 'typescript',
      content: 'const value = 2\n',
      savedContent: 'const value = 1\n',
    }, 'const value = 3\n')

    expect(resolution.issue).toBe('conflict')
    expect(resolution.tab.content).toBe('const value = 2\n')
    expect(resolution.tab.savedContent).toBe('const value = 3\n')
    expect(resolution.tab.recoveryState).toBe('conflict')
  })

  it('reloads clean tabs when the disk version changes externally', () => {
    const resolution = syncTabWithDisk(createTab({
      content: 'const value = 1\n',
      savedContent: 'const value = 1\n',
    }), true, 'const value = 4\n')

    expect(resolution.effect).toBe('reloaded')
    expect(resolution.tab.content).toBe('const value = 4\n')
    expect(resolution.tab.savedContent).toBe('const value = 4\n')
  })

  it('marks dirty tabs as conflicted when the disk version changes underneath them', () => {
    const resolution = syncTabWithDisk(createTab(), true, 'const value = 5\n')

    expect(resolution.effect).toBe('conflict')
    expect(resolution.tab.savedContent).toBe('const value = 5\n')
    expect(resolution.tab.recoveryState).toBe('conflict')
  })
})
