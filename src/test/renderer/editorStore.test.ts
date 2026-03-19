import { beforeEach, describe, expect, it } from 'vitest'
import type { Tab } from '@shared/types/file'
import { useEditorStore } from '../../renderer/store/editorStore'

function createTab(id: string, name = `${id}.ts`): Tab {
  return {
    id,
    path: `C:\\workspace\\${name}`,
    name,
    content: 'console.log("hello")\n',
    savedContent: 'console.log("hello")\n',
    language: 'typescript',
  }
}

function resetEditorStore() {
  useEditorStore.setState({
    tabs: [],
    recentFiles: [],
    closedTabs: [],
    groups: [{ id: 'main', tabIds: [], activeTabId: null }],
    activeGroupId: 'main',
    layoutDirection: 'horizontal',
  })
}

describe('editorStore split editor behavior', () => {
  beforeEach(() => {
    resetEditorStore()
  })

  it('duplicates the active tab into a new split group and focuses it', () => {
    const tab = createTab('tab-1', 'app.ts')

    useEditorStore.setState({
      tabs: [tab],
      groups: [{ id: 'main', tabIds: [tab.id], activeTabId: tab.id }],
      activeGroupId: 'main',
      layoutDirection: 'horizontal',
    })

    useEditorStore.getState().splitGroup('main', 'horizontal')

    const state = useEditorStore.getState()
    expect(state.groups).toHaveLength(2)
    expect(state.groups[0]).toMatchObject({ id: 'main', tabIds: [tab.id], activeTabId: tab.id })
    expect(state.groups[1]).toMatchObject({ tabIds: [tab.id], activeTabId: tab.id })
    expect(state.activeGroupId).toBe(state.groups[1].id)
    expect(state.layoutDirection).toBe('horizontal')
  })

  it('removes an empty secondary split while keeping tabs still referenced elsewhere', () => {
    const tab = createTab('tab-1', 'shared.ts')

    useEditorStore.setState({
      tabs: [tab],
      groups: [
        { id: 'main', tabIds: [tab.id], activeTabId: tab.id },
        { id: 'group-2', tabIds: [tab.id], activeTabId: tab.id },
      ],
      activeGroupId: 'group-2',
      layoutDirection: 'horizontal',
    })

    useEditorStore.getState().closeTab(tab.id, 'group-2')

    const state = useEditorStore.getState()
    expect(state.groups).toHaveLength(1)
    expect(state.groups[0].id).toBe('main')
    expect(state.tabs).toHaveLength(1)
    expect(state.tabs[0].id).toBe(tab.id)
    expect(state.activeGroupId).toBe('main')
    expect(state.layoutDirection).toBe('horizontal')
  })

  it('drops unreferenced tabs and resets layout when closing the last extra split', () => {
    const mainTab = createTab('tab-1', 'main.ts')
    const splitTab = createTab('tab-2', 'secondary.ts')

    useEditorStore.setState({
      tabs: [mainTab, splitTab],
      groups: [
        { id: 'main', tabIds: [mainTab.id], activeTabId: mainTab.id },
        { id: 'group-2', tabIds: [splitTab.id], activeTabId: splitTab.id },
      ],
      activeGroupId: 'group-2',
      layoutDirection: 'vertical',
    })

    useEditorStore.getState().closeGroup('group-2')

    const state = useEditorStore.getState()
    expect(state.groups).toHaveLength(1)
    expect(state.groups[0]).toMatchObject({ id: 'main', tabIds: [mainTab.id], activeTabId: mainTab.id })
    expect(state.tabs).toHaveLength(1)
    expect(state.tabs[0].id).toBe(mainTab.id)
    expect(state.activeGroupId).toBe('main')
    expect(state.layoutDirection).toBe('horizontal')
  })

  it('restores the most recently closed tab into the active group', () => {
    const tab = createTab('tab-1', 'notes.ts')

    useEditorStore.setState({
      tabs: [tab],
      recentFiles: [],
      closedTabs: [],
      groups: [{ id: 'main', tabIds: [tab.id], activeTabId: tab.id }],
      activeGroupId: 'main',
      layoutDirection: 'horizontal',
    })

    useEditorStore.getState().closeTab(tab.id, 'main')

    expect(useEditorStore.getState().closedTabs[0]?.id).toBe(tab.id)

    const reopened = useEditorStore.getState().reopenClosedTab('main')
    const state = useEditorStore.getState()

    expect(reopened).toBe(true)
    expect(state.groups[0]).toMatchObject({ tabIds: [tab.id], activeTabId: tab.id })
    expect(state.tabs[0]).toMatchObject({ id: tab.id, content: tab.content })
    expect(state.closedTabs).toHaveLength(0)
    expect(state.recentFiles[0]).toBe(tab.path)
  })
})
