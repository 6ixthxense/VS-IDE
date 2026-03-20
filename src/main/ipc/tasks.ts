import { ipcMain } from 'electron'
import { IPC } from '../../shared/constants'
import type { OperationResult, TaskRunRequest } from '../../shared/types/ipc'
import { taskService } from '../services/tasks'
import { workspaceService } from '../services/workspace'

function failedOperation(error: unknown, fallback: string): OperationResult {
  return {
    success: false,
    error: fallback,
    details: error instanceof Error ? error.message : String(error),
  }
}

function sanitizeTaskRequest(request: TaskRunRequest): TaskRunRequest {
  const cwd = request.task.cwd
    ? workspaceService.assertWithinWorkspace(request.task.cwd, 'Task cwd')
    : workspaceService.requireRootPath()

  return {
    ...request,
    task: {
      ...request.task,
      cwd,
    },
  }
}

export function registerTaskHandlers() {
  ipcMain.handle(IPC.TASKS_LIST, async () => {
    try {
      return taskService.listTaskDefinitions(workspaceService.requireRootPath())
    } catch {
      return []
    }
  })

  ipcMain.handle(IPC.TASKS_RUN, async (_, _rootPath: string, request: TaskRunRequest) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      return taskService.runTask(rootPath, sanitizeTaskRequest(request))
    } catch (error) {
      return failedOperation(error, 'Unable to start this task')
    }
  })

  ipcMain.handle(IPC.TASKS_STOP, async (_, runId: string) => {
    try {
      return taskService.stopTask(runId)
    } catch (error) {
      return failedOperation(error, 'Task could not be stopped')
    }
  })
}
