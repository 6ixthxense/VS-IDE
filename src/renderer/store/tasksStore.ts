import { create } from 'zustand'
import type { TaskDefinition, TaskEvent, TaskRun } from '@shared/types/ipc'
import { showErrorToast, showSuccessToast } from './feedbackStore'
import { useTerminalStore } from './terminalStore'
import { useUiStore } from './uiStore'

const STORAGE_PREFIX = 'vs-monitor-ide'
const MAX_RECENT_TASKS = 10
const MAX_TASK_RUNS = 18

interface PersistedTasksState {
  recentTasks: TaskDefinition[]
}

interface TasksState {
  workspaceRoot: string | null
  definitions: TaskDefinition[]
  recentTasks: TaskDefinition[]
  runs: TaskRun[]
  loadingDefinitions: boolean
  restoreForWorkspace: (rootPath: string | null) => Promise<void>
  loadDefinitions: () => Promise<void>
  runTask: (task: TaskDefinition) => Promise<TaskRun | null>
  stopTask: (runId: string) => Promise<void>
  retryTask: (runId: string) => Promise<void>
  handleTaskEvent: (event: TaskEvent) => void
}

function normalizeWorkspaceKey(rootPath: string) {
  return encodeURIComponent(rootPath.replace(/\\/g, '/').toLowerCase())
}

function buildTasksKey(rootPath: string) {
  return `${STORAGE_PREFIX}:workspace:${normalizeWorkspaceKey(rootPath)}:tasks`
}

function readTasksSnapshot(rootPath: string): PersistedTasksState | null {
  if (typeof window === 'undefined') return null

  try {
    const raw = window.localStorage.getItem(buildTasksKey(rootPath))
    return raw ? JSON.parse(raw) as PersistedTasksState : null
  } catch {
    return null
  }
}

function writeTasksSnapshot(rootPath: string, snapshot: PersistedTasksState) {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(buildTasksKey(rootPath), JSON.stringify(snapshot))
}

function persistTasksState(state: Pick<TasksState, 'workspaceRoot' | 'recentTasks'>) {
  if (!state.workspaceRoot) return
  writeTasksSnapshot(state.workspaceRoot, { recentTasks: state.recentTasks })
}

function touchRecentTask(items: TaskDefinition[], task: TaskDefinition) {
  return [task, ...items.filter((item) => item.id !== task.id)].slice(0, MAX_RECENT_TASKS)
}

function upsertRun(items: TaskRun[], nextRun: TaskRun) {
  const nextItems = [nextRun, ...items.filter((item) => item.runId !== nextRun.runId)]
  return nextItems
    .sort((left, right) => Date.parse(right.startedAt) - Date.parse(left.startedAt))
    .slice(0, MAX_TASK_RUNS)
}

function ensureTerminalVisible() {
  const uiState = useUiStore.getState()
  if (!uiState.showTerminal) {
    uiState.toggleTerminal()
  }
}

function buildFallbackTaskDefinition(run: TaskRun): TaskDefinition {
  return {
    id: run.taskId,
    label: run.label,
    command: run.command,
    args: [],
    source: run.source,
    cwd: run.cwd,
    detail: run.detail,
    shell: true,
  }
}

export const useTasksStore = create<TasksState>((set, get) => ({
  workspaceRoot: null,
  definitions: [],
  recentTasks: [],
  runs: [],
  loadingDefinitions: false,

  restoreForWorkspace: async (rootPath) => {
    if (!rootPath) {
      set({
        workspaceRoot: null,
        definitions: [],
        recentTasks: [],
        runs: [],
        loadingDefinitions: false,
      })
      return
    }

    const snapshot = readTasksSnapshot(rootPath)
    set({
      workspaceRoot: rootPath,
      definitions: [],
      recentTasks: snapshot?.recentTasks ?? [],
      runs: [],
      loadingDefinitions: false,
    })

    await get().loadDefinitions()
  },

  loadDefinitions: async () => {
    const rootPath = get().workspaceRoot
    if (!rootPath) return

    set({ loadingDefinitions: true })

    try {
      const definitions = await window.electronAPI.getTaskDefinitions(rootPath)
      set({ definitions, loadingDefinitions: false })
    } catch (error) {
      set({ loadingDefinitions: false })
      showErrorToast((error as Error).message || 'Unable to load package scripts.', 'Task discovery failed')
    }
  },

  runTask: async (task) => {
    const rootPath = get().workspaceRoot
    if (!rootPath) return null

    ensureTerminalVisible()

    const terminalState = useTerminalStore.getState()
    const terminalId = terminalState.activeId || 'default'
    const cwd = task.cwd || rootPath

    terminalState.updateTerminalCwd(terminalId, cwd)
    window.electronAPI.setTerminalCwd(terminalId, cwd)

    const nextTask = {
      ...task,
      cwd,
    }

    const result = await window.electronAPI.runTask(rootPath, {
      terminalId,
      task: nextTask,
    })

    if (!result.success || !result.run) {
      showErrorToast(result.error || 'Unable to start this task.', 'Task start failed', result.details)
      return null
    }

    set((state) => {
      const recentTasks = touchRecentTask(state.recentTasks, nextTask)
      const nextState = {
        recentTasks,
        runs: upsertRun(state.runs, result.run!),
      }
      persistTasksState({ ...state, ...nextState })
      return nextState
    })

    showSuccessToast(`Running "${nextTask.label}" in terminal ${terminalId}.`, 'Task started')
    return result.run
  },

  stopTask: async (runId) => {
    const result = await window.electronAPI.stopTask(runId)
    if (!result.success) {
      showErrorToast(result.error || 'Unable to stop this task.', 'Stop task failed', result.details)
    }
  },

  retryTask: async (runId) => {
    const state = get()
    const run = state.runs.find((entry) => entry.runId === runId)
    if (!run) {
      showErrorToast('The selected task run is no longer available.', 'Retry failed')
      return
    }

    const task = state.definitions.find((entry) => entry.id === run.taskId)
      ?? state.recentTasks.find((entry) => entry.id === run.taskId)
      ?? buildFallbackTaskDefinition(run)

    await state.runTask(task)
  },

  handleTaskEvent: (event) => {
    set((state) => ({
      runs: upsertRun(state.runs, event),
    }))
  },
}))
