import { create } from 'zustand'
import type { ProblemScanResult, ProblemSeverity, ProblemSource } from '@shared/types/ipc'
import { showErrorToast } from './feedbackStore'

type ProblemSeverityFilter = 'all' | ProblemSeverity
type ProblemSourceFilter = 'all' | ProblemSource

interface PersistedProblemsState {
  result: ProblemScanResult | null
  severityFilter: ProblemSeverityFilter
  sourceFilter: ProblemSourceFilter
}

interface ProblemsState {
  workspaceRoot: string | null
  result: ProblemScanResult | null
  liveEntries: ProblemScanResult['entries']
  loading: boolean
  severityFilter: ProblemSeverityFilter
  sourceFilter: ProblemSourceFilter
  restoreForWorkspace: (rootPath: string | null) => Promise<void>
  scan: () => Promise<ProblemScanResult | null>
  setLiveEntries: (rootPath: string, entries: ProblemScanResult['entries']) => void
  setSeverityFilter: (filter: ProblemSeverityFilter) => void
  setSourceFilter: (filter: ProblemSourceFilter) => void
}

const STORAGE_PREFIX = 'vs-monitor-ide'

function normalizeWorkspaceKey(rootPath: string) {
  return encodeURIComponent(rootPath.replace(/\\/g, '/').toLowerCase())
}

function buildProblemsKey(rootPath: string) {
  return `${STORAGE_PREFIX}:workspace:${normalizeWorkspaceKey(rootPath)}:problems`
}

function readProblemsSnapshot(rootPath: string): PersistedProblemsState | null {
  if (typeof window === 'undefined') return null

  try {
    const raw = window.localStorage.getItem(buildProblemsKey(rootPath))
    return raw ? JSON.parse(raw) as PersistedProblemsState : null
  } catch {
    return null
  }
}

function writeProblemsSnapshot(rootPath: string, snapshot: PersistedProblemsState) {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(buildProblemsKey(rootPath), JSON.stringify(snapshot))
}

function persistProblemsState(state: Pick<ProblemsState, 'workspaceRoot' | 'result' | 'severityFilter' | 'sourceFilter'>) {
  if (!state.workspaceRoot) return

  writeProblemsSnapshot(state.workspaceRoot, {
    result: state.result,
    severityFilter: state.severityFilter,
    sourceFilter: state.sourceFilter,
  })
}

function shouldUseLatestResult(latest: ProblemScanResult, existing: ProblemScanResult | null) {
  if (!existing) {
    return latest.entries.length > 0 || Boolean(latest.error) || (latest.notes?.length ?? 0) > 0
  }

  return Date.parse(latest.scannedAt) >= Date.parse(existing.scannedAt)
}

export const useProblemsStore = create<ProblemsState>((set, get) => ({
  workspaceRoot: null,
  result: null,
  liveEntries: [],
  loading: false,
  severityFilter: 'all',
  sourceFilter: 'all',

  restoreForWorkspace: async (rootPath) => {
    if (!rootPath) {
      set({
        workspaceRoot: null,
        result: null,
        liveEntries: [],
        loading: false,
        severityFilter: 'all',
        sourceFilter: 'all',
      })
      return
    }

    const snapshot = readProblemsSnapshot(rootPath)
    set({
      workspaceRoot: rootPath,
      result: snapshot?.result ?? null,
      liveEntries: [],
      loading: false,
      severityFilter: snapshot?.severityFilter ?? 'all',
      sourceFilter: snapshot?.sourceFilter ?? 'all',
    })

    try {
      const latest = await window.electronAPI.getLastProblems(rootPath)
      if (!shouldUseLatestResult(latest, snapshot?.result ?? null)) {
        return
      }

      set((state) => {
        const nextState = { result: latest }
        persistProblemsState({ ...state, ...nextState })
        return nextState
      })
    } catch {
      // Keep the local snapshot if IPC is unavailable.
    }
  },

  scan: async () => {
    const rootPath = get().workspaceRoot
    if (!rootPath) return null

    set({ loading: true })

    try {
      const result = await window.electronAPI.scanProblems(rootPath)
      set((state) => {
        const nextState = { result, loading: false }
        persistProblemsState({ ...state, ...nextState })
        return nextState
      })
      return result
    } catch (error) {
      set({ loading: false })
      showErrorToast((error as Error).message || 'Unable to scan workspace problems.', 'Problem scan failed')
      return null
    }
  },

  setLiveEntries: (rootPath, liveEntries) => {
    set((state) => {
      if (state.workspaceRoot && state.workspaceRoot !== rootPath) {
        return state
      }

      return { liveEntries }
    })
  },

  setSeverityFilter: (severityFilter) => {
    set((state) => {
      const nextState = { severityFilter }
      persistProblemsState({ ...state, ...nextState })
      return nextState
    })
  },

  setSourceFilter: (sourceFilter) => {
    set((state) => {
      const nextState = { sourceFilter }
      persistProblemsState({ ...state, ...nextState })
      return nextState
    })
  },
}))
