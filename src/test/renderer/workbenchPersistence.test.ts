import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { Tab } from '@shared/types/file'
import {
  readEditorSession,
  readWorkbenchUi,
  serializeTabsForSession,
  writeEditorSession,
  writeWorkbenchUi,
} from '../../renderer/utils/workbenchPersistence'

function createStorage() {
  const map = new Map<string, string>()

  return {
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value)
    },
    removeItem: (key: string) => {
      map.delete(key)
    },
    clear: () => {
      map.clear()
    },
  }
}

function createTab(id: string, content: string, savedContent = content): Tab {
  return {
    id,
    path: `C:\\workspace\\${id}.ts`,
    name: `${id}.ts`,
    language: 'typescript',
    content,
    savedContent,
  }
}

describe('workbenchPersistence', () => {
  const originalWindow = globalThis.window

  beforeEach(() => {
    const localStorage = createStorage()
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: { localStorage },
    })
  })

  afterEach(() => {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: originalWindow,
    })
  })

  it('keeps editor snapshots scoped to the workspace path', () => {
    writeEditorSession('C:\\workspace-a', {
      tabs: [{ id: 'a', path: 'C:\\workspace-a\\a.ts', name: 'a.ts', language: 'typescript' }],
      groups: [{ id: 'main', tabIds: ['a'], activeTabId: 'a' }],
      activeGroupId: 'main',
      layoutDirection: 'horizontal',
    })

    writeEditorSession('C:\\workspace-b', {
      tabs: [{ id: 'b', path: 'C:\\workspace-b\\b.ts', name: 'b.ts', language: 'typescript' }],
      groups: [{ id: 'main', tabIds: ['b'], activeTabId: 'b' }],
      activeGroupId: 'main',
      layoutDirection: 'horizontal',
    })

    expect(readEditorSession('C:\\workspace-a')?.tabs[0]?.id).toBe('a')
    expect(readEditorSession('C:\\workspace-b')?.tabs[0]?.id).toBe('b')
  })

  it('stores global UI preferences independently from workspace snapshots', () => {
    writeWorkbenchUi({
      showTerminal: false,
      showSysMonitor: true,
      activeSidebarView: 'git',
      showSidebar: true,
    })

    expect(readWorkbenchUi()).toEqual({
      showTerminal: false,
      showSysMonitor: true,
      activeSidebarView: 'git',
      showSidebar: true,
    })
  })

  it('drops oversized tab content while keeping the tab metadata restorable', () => {
    const smallTab = createTab('small', 'console.log("ok")\n')
    const largeText = 'x'.repeat(240_000)
    const largeTab = createTab('large', largeText, largeText)

    const snapshot = serializeTabsForSession([smallTab, largeTab])

    expect(snapshot[0]).toMatchObject({ id: 'small', content: 'console.log("ok")\n' })
    expect(snapshot[1]).toMatchObject({ id: 'large', content: undefined, savedContent: undefined })
    expect(snapshot[1].path).toBe('C:\\workspace\\large.ts')
  })
})
