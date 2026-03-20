import { BrowserWindow } from 'electron'
import * as childProcess from 'child_process'
import fs from 'fs'
import path from 'path'
import { IPC } from '../../shared/constants'
import type { OperationResult, TaskDefinition, TaskEvent, TaskRun, TaskRunRequest, TaskRunStatus } from '../../shared/types/ipc'
import { diagnosticsService } from './diagnostics'

interface ActiveTaskRun {
  child: childProcess.ChildProcess
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

function hasChildProcessExited(child: childProcess.ChildProcess) {
  return child.exitCode != null || child.signalCode != null || child.killed
}

function waitForChildPid(child: childProcess.ChildProcess, timeoutMs = 1500): Promise<number | null> {
  if (typeof child.pid === 'number' && child.pid > 0) {
    return Promise.resolve(child.pid)
  }

  return new Promise((resolve) => {
    let settled = false

    const cleanup = () => {
      child.removeListener('spawn', handleDone)
      child.removeListener('error', handleDone)
      child.removeListener('exit', handleDone)
      child.removeListener('close', handleDone)
      clearTimeout(timeoutId)
    }

    const settle = () => {
      if (settled) return
      settled = true
      cleanup()
      resolve(typeof child.pid === 'number' && child.pid > 0 ? child.pid : null)
    }

    const handleDone = () => settle()
    const timeoutId = setTimeout(() => settle(), timeoutMs)

    child.once('spawn', handleDone)
    child.once('error', handleDone)
    child.once('exit', handleDone)
    child.once('close', handleDone)
  })
}

function waitForChildExit(child: childProcess.ChildProcess, timeoutMs = 1200): Promise<boolean> {
  if (hasChildProcessExited(child)) {
    return Promise.resolve(true)
  }

  return new Promise((resolve) => {
    let settled = false

    const cleanup = () => {
      child.removeListener('exit', handleDone)
      child.removeListener('close', handleDone)
      clearTimeout(timeoutId)
    }

    const settle = (value: boolean) => {
      if (settled) return
      settled = true
      cleanup()
      resolve(value)
    }

    const handleDone = () => settle(true)
    const timeoutId = setTimeout(() => settle(hasChildProcessExited(child)), timeoutMs)

    child.once('exit', handleDone)
    child.once('close', handleDone)
  })
}

function runHelperProcess(
  spawnProcess: typeof childProcess.spawn,
  command: string,
  args: string[]
) {
  return new Promise<{ exitCode: number | null; stdout: string; stderr: string }>((resolve, reject) => {
    const helper = spawnProcess(command, args, {
      windowsHide: true,
      shell: false,
      env: process.env,
    })

    let stdout = ''
    let stderr = ''

    helper.stdout.on('data', (chunk) => {
      stdout += chunk.toString()
    })

    helper.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })

    helper.on('error', reject)
    helper.on('close', (exitCode) => {
      resolve({ exitCode, stdout, stderr })
    })
  })
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

  public constructor(
    private readonly spawnProcess: typeof childProcess.spawn = childProcess.spawn
  ) {}

  private async terminateChildProcess(child: childProcess.ChildProcess): Promise<OperationResult> {
    if (hasChildProcessExited(child)) {
      return { success: true }
    }

    const pid = await waitForChildPid(child)
    if (!pid) {
      return { success: true }
    }

    if (process.platform === 'win32') {
      try {
        const helperResult = await runHelperProcess(this.spawnProcess, 'taskkill', ['/pid', String(pid), '/t', '/f'])
        const helperOutput = `${helperResult.stdout}\n${helperResult.stderr}`.trim()
        const processAlreadyGone = /not found|no running instance|cannot find the process/i.test(helperOutput)

        if (helperResult.exitCode === 0 || processAlreadyGone || hasChildProcessExited(child)) {
          return { success: true }
        }

        return {
          success: false,
          error: 'Task could not be stopped',
          details: helperOutput || `taskkill exited with code ${helperResult.exitCode ?? 'unknown'}`,
        }
      } catch (error) {
        return toOperationFailure(error, 'Task could not be stopped')
      }
    }

    try {
      child.kill('SIGTERM')
    } catch (error) {
      return toOperationFailure(error, 'Task could not be stopped')
    }

    if (await waitForChildExit(child)) {
      return { success: true }
    }

    try {
      child.kill('SIGKILL')
    } catch (error) {
      return toOperationFailure(error, 'Task could not be stopped')
    }

    return (await waitForChildExit(child))
      ? { success: true }
      : { success: false, error: 'Task could not be stopped' }
  }

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

      const child = this.spawnProcess(request.task.command, request.task.args, {
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

  public async stopTask(runId: string): Promise<OperationResult> {
    const activeRun = this.activeRuns.get(runId)
    if (!activeRun) {
      return {
        success: false,
        error: 'Task is no longer running',
      }
    }

    activeRun.stoppedByUser = true

    if (hasChildProcessExited(activeRun.child)) {
      return { success: true }
    }

    return await this.terminateChildProcess(activeRun.child)
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
