import { BrowserWindow } from 'electron'
import { spawn, type ChildProcess } from 'child_process'
import fs from 'fs'
import path from 'path'
import { IPC } from '../../shared/constants'
import type { OperationResult, TaskDefinition, TaskEvent, TaskRun, TaskRunRequest, TaskRunStatus } from '../../shared/types/ipc'
import { diagnosticsService } from './diagnostics'

interface ActiveTaskRun {
  child: ChildProcess
  run: TaskRun
  stoppedByUser: boolean
  finished: boolean
}

function toOperationFailure(error: unknown, fallback: string): OperationResult {
  return {
    success: false,
    error: fallback,
    details: error instanceof Error ? error.message : String(error),
  }
}

function normalizeTerminalText(value: string) {
  return value.replace(/\r?\n/g, '\r\n')
}

function sendTerminalOutput(terminalId: string, data: string) {
  BrowserWindow.getAllWindows().forEach((win) => {
    if (!win.isDestroyed()) {
      win.webContents.send(IPC.TERMINAL_OUT, { id: terminalId, data })
    }
  })
}

function broadcastTaskEvent(event: TaskEvent) {
  BrowserWindow.getAllWindows().forEach((win) => {
    if (!win.isDestroyed()) {
      win.webContents.send(IPC.TASKS_EVENT, event)
    }
  })
}

function buildTaskCommand(task: TaskDefinition) {
  return task.args.length > 0 ? `${task.command} ${task.args.join(' ')}` : task.command
}

export function discoverPackageScriptTasks(rootPath: string): TaskDefinition[] {
  const packageJsonPath = path.join(rootPath, 'package.json')
  if (!fs.existsSync(packageJsonPath)) {
    return []
  }

  try {
    const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8')) as { scripts?: Record<string, string> }
    const scripts = packageJson.scripts ?? {}
    const npmCommand = process.platform === 'win32' ? 'npm.cmd' : 'npm'

    return Object.entries(scripts)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([scriptName, scriptCommand]) => ({
        id: `npm:${scriptName}`,
        label: scriptName,
        command: npmCommand,
        args: ['run', scriptName],
        source: 'package-script' as const,
        cwd: rootPath,
        detail: scriptCommand,
        shell: process.platform === 'win32',
      }))
  } catch {
    return []
  }
}

export class TaskService {
  private readonly activeRuns = new Map<string, ActiveTaskRun>()
  private readonly activeRunByTerminal = new Map<string, string>()

  public listTaskDefinitions(rootPath: string) {
    return discoverPackageScriptTasks(rootPath)
  }

  public runTask(rootPath: string, request: TaskRunRequest): OperationResult & { run?: TaskRun } {
    try {
      const cwd = request.task.cwd || rootPath
      const existingRunId = this.activeRunByTerminal.get(request.terminalId)
      if (existingRunId) {
        void this.stopTask(existingRunId)
      }

      const run: TaskRun = {
        runId: `task-run-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        taskId: request.task.id,
        label: request.task.label,
        command: buildTaskCommand(request.task),
        source: request.task.source,
        terminalId: request.terminalId,
        cwd,
        status: 'running',
        startedAt: new Date().toISOString(),
        detail: request.task.detail,
      }

      sendTerminalOutput(run.terminalId, `\r\n\x1b[35m[Task] ${run.label}\x1b[0m\r\n`)
      sendTerminalOutput(run.terminalId, `\x1b[90m${run.command}\x1b[0m\r\n`)

      const child = spawn(request.task.command, request.task.args, {
        cwd,
        env: process.env,
        shell: request.task.shell ?? process.platform === 'win32',
        windowsHide: true,
      })

      const activeRun: ActiveTaskRun = {
        child,
        run,
        stoppedByUser: false,
        finished: false,
      }

      this.activeRuns.set(run.runId, activeRun)
      this.activeRunByTerminal.set(run.terminalId, run.runId)
      broadcastTaskEvent({
        ...run,
        message: `${run.label} started.`,
      })

      child.stdout.on('data', (chunk) => {
        sendTerminalOutput(run.terminalId, normalizeTerminalText(chunk.toString()))
      })

      child.stderr.on('data', (chunk) => {
        sendTerminalOutput(run.terminalId, `\x1b[31m${normalizeTerminalText(chunk.toString())}\x1b[0m`)
      })

      child.on('error', (error) => {
        this.finishRun(run.runId, 'error', error instanceof Error ? error.message : String(error), null)
      })

      child.on('close', (exitCode) => {
        const nextStatus: TaskRunStatus = activeRun.stoppedByUser
          ? 'cancelled'
          : exitCode === 0
            ? 'success'
            : 'error'
        const message = activeRun.stoppedByUser
          ? `${run.label} was cancelled.`
          : exitCode === 0
            ? `${run.label} finished successfully.`
            : `${run.label} exited with code ${exitCode}.`

        this.finishRun(run.runId, nextStatus, message, exitCode)
      })

      return { success: true, run }
    } catch (error) {
      return toOperationFailure(error, 'Unable to start this task')
    }
  }

  public stopTask(runId: string): OperationResult {
    const activeRun = this.activeRuns.get(runId)
    if (!activeRun) {
      return {
        success: false,
        error: 'Task is no longer running',
      }
    }

    activeRun.stoppedByUser = true

    try {
      const killed = activeRun.child.kill()
      if (!killed) {
        return {
          success: false,
          error: 'Task could not be stopped',
        }
      }

      return { success: true }
    } catch (error) {
      return toOperationFailure(error, 'Task could not be stopped')
    }
  }

  private finishRun(runId: string, status: TaskRunStatus, message: string, exitCode: number | null) {
    const activeRun = this.activeRuns.get(runId)
    if (!activeRun || activeRun.finished) {
      return
    }

    activeRun.finished = true
    const nextRun: TaskRun = {
      ...activeRun.run,
      status,
      finishedAt: new Date().toISOString(),
      exitCode,
    }

    this.activeRuns.delete(runId)
    if (this.activeRunByTerminal.get(nextRun.terminalId) === runId) {
      this.activeRunByTerminal.delete(nextRun.terminalId)
    }

    if (status === 'success') {
      diagnosticsService.info('tasks', 'Task finished successfully.', `${nextRun.label}\n${nextRun.command}`)
      sendTerminalOutput(nextRun.terminalId, `\r\n\x1b[32m[Done] ${message}\x1b[0m\r\n`)
    } else if (status === 'cancelled') {
      diagnosticsService.warn('tasks', 'Task cancelled.', `${nextRun.label}\n${nextRun.command}`)
      sendTerminalOutput(nextRun.terminalId, `\r\n\x1b[33m[Cancelled] ${message}\x1b[0m\r\n`)
    } else {
      diagnosticsService.error('tasks', 'Task finished with errors.', `${nextRun.label}\n${nextRun.command}\n${message}`)
      sendTerminalOutput(nextRun.terminalId, `\r\n\x1b[31m[Failed] ${message}\x1b[0m\r\n`)
    }

    broadcastTaskEvent({
      ...nextRun,
      message,
    })
  }
}

export const taskService = new TaskService()
