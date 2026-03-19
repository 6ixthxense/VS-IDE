import type { Tab } from '@shared/types/file'
import type { PersistedEditorTab } from './workbenchPersistence'

export const RECOVERED_CHANGES_MESSAGE = 'Recovered unsaved changes from your previous session.'
export const CONFLICT_ON_RESTORE_MESSAGE = 'This file changed on disk while the app was closed.'
export const CONFLICT_ON_SYNC_MESSAGE = 'This file changed on disk while you still have unsaved edits.'
export const FILE_MISSING_MESSAGE = 'This file is no longer available on disk.'

type RestoredIssue = 'recovered' | 'conflict' | null
type SyncEffect = 'unchanged' | 'saved' | 'reloaded' | 'conflict' | 'missing'

export interface RestoredTabResolution {
  issue: RestoredIssue
  tab: Tab
}

export interface SyncedTabResolution {
  effect: SyncEffect
  tab: Tab
}

export function resolveRestoredTab(snapshot: PersistedEditorTab, diskContent: string): RestoredTabResolution {
  const persistedContent = snapshot.content ?? diskContent
  const persistedSavedContent = snapshot.savedContent ?? diskContent
  const hasUnsavedChanges = persistedContent !== persistedSavedContent

  if (!hasUnsavedChanges || diskContent === persistedContent) {
    return {
      issue: null,
      tab: {
        id: snapshot.id,
        path: snapshot.path,
        name: snapshot.name,
        language: snapshot.language,
        content: diskContent,
        savedContent: diskContent,
      },
    }
  }

  if (diskContent === persistedSavedContent) {
    return {
      issue: 'recovered',
      tab: {
        id: snapshot.id,
        path: snapshot.path,
        name: snapshot.name,
        language: snapshot.language,
        content: persistedContent,
        savedContent: diskContent,
        recoveryState: 'recovered',
        recoveryMessage: RECOVERED_CHANGES_MESSAGE,
      },
    }
  }

  return {
    issue: 'conflict',
    tab: {
      id: snapshot.id,
      path: snapshot.path,
      name: snapshot.name,
      language: snapshot.language,
      content: persistedContent,
      savedContent: diskContent,
      recoveryState: 'conflict',
      recoveryMessage: CONFLICT_ON_RESTORE_MESSAGE,
    },
  }
}

export function syncTabWithDisk(tab: Tab, diskExists: boolean, diskContent: string): SyncedTabResolution {
  if (!diskExists) {
    return {
      effect: 'missing',
      tab: {
        ...tab,
        recoveryState: 'conflict',
        recoveryMessage: FILE_MISSING_MESSAGE,
      },
    }
  }

  if (diskContent === tab.savedContent) {
    return { effect: 'unchanged', tab }
  }

  if (diskContent === tab.content) {
    return {
      effect: 'saved',
      tab: {
        ...tab,
        savedContent: diskContent,
        recoveryState: undefined,
        recoveryMessage: undefined,
      },
    }
  }

  if (tab.content === tab.savedContent) {
    return {
      effect: 'reloaded',
      tab: {
        ...tab,
        content: diskContent,
        savedContent: diskContent,
        recoveryState: undefined,
        recoveryMessage: undefined,
      },
    }
  }

  return {
    effect: 'conflict',
    tab: {
      ...tab,
      savedContent: diskContent,
      recoveryState: 'conflict',
      recoveryMessage: CONFLICT_ON_SYNC_MESSAGE,
    },
  }
}
