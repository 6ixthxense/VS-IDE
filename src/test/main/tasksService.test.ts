import type { ChildProcess } from 'child_process'
import { EventEmitter } from 'events'
import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { TaskService, discoverPackageScriptTasks } from '../../main/services/tasks'

function createWorkspaceWithPackageJson(scripts: Record<string, string>) {
  const rootPath = fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-tasks-'))
  fs.writeFileSync(
    path.join(rootPath, 'package.json'),
    JSON.stringify({ name: 'sample', scripts }, null, 2),
    'utf8'
  )
  return rootPath
}

function createMockChild(overrides: Partial<ChildProcess> = {}) {
  const child = new EventEmitter() as ChildProcess & EventEmitter
  ;(child as any).stdout = new EventEmitter()
  ;(child as any).stderr = new EventEmitter()
  ;(child as any).kill = vi.fn(() => true)
  ;(child as any).pid = 1234
  ;(child as any).exitCode = null
  ;(child as any).signalCode = null
  ;(child as any).killed = false
  return Object.assign(child, overrides)
}

function createTaskRun() {
  return {
    runId: 'run-1',
    taskId: 'npm:build',
    label: 'build',
    command: 'npm.cmd run build',
    source: 'package-script' as const,
    terminalId: 'default',
    cwd: 'C:\\workspace',
    status: 'running' as const,
    startedAt: new Date().toISOString(),
  }
}

const runWindowsSpecific = process.platform === 'win32' ? it : it.skip

afterEach(() => {
  vi.restoreAllMocks()
})

describe('taskService discovery', () => {
  it('returns npm script definitions from package.json', () => {
    const rootPath = createWorkspaceWithPackageJson({
      build: 'vite build',
      dev: 'vite',
      test: 'vitest run',
    })

    const tasks = discoverPackageScriptTasks(rootPath)

    expect(tasks).toEqual([
      expect.objectContaining({
        id: 'npm:build',
        label: 'build',
        command: process.platform === 'win32' ? 'npm.cmd' : 'npm',
        args: ['run', 'build'],
        source: 'package-script',
        cwd: rootPath,
        detail: 'vite build',
      }),
      expect.objectContaining({
        id: 'npm:dev',
        label: 'dev',
        args: ['run', 'dev'],
        detail: 'vite',
      }),
      expect.objectContaining({
        id: 'npm:test',
        label: 'test',
        args: ['run', 'test'],
        detail: 'vitest run',
      }),
    ])
  })

  it('returns an empty array when package.json is missing or invalid', () => {
    const emptyRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-tasks-empty-'))
    expect(discoverPackageScriptTasks(emptyRoot)).toEqual([])

    const invalidRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-tasks-invalid-'))
    fs.writeFileSync(path.join(invalidRoot, 'package.json'), '{ invalid json', 'utf8')
    expect(discoverPackageScriptTasks(invalidRoot)).toEqual([])
  })

  runWindowsSpecific('stops a running task by terminating its process tree', async () => {
    const helper = createMockChild({ pid: 9999 })
    const spawnMock = vi.fn(() => {
      queueMicrotask(() => {
        helper.emit('close', 0)
      })
      return helper
    })

    const service = new TaskService(spawnMock as never) as any
    const child = createMockChild({ pid: 4321, kill: vi.fn(() => false) })
    service.activeRuns.set('run-1', {
      child,
      run: createTaskRun(),
      stoppedByUser: false,
      finished: false,
    })

    const result = await service.stopTask('run-1')

    expect(result).toEqual({ success: true })
    expect(spawnMock).toHaveBeenCalledWith('taskkill', ['/pid', '4321', '/t', '/f'], expect.any(Object))
    expect(service.activeRuns.get('run-1').stoppedByUser).toBe(true)
  })

  runWindowsSpecific('waits briefly for a pid when stop is pressed immediately after launch', async () => {
    const helper = createMockChild({ pid: 9999 })
    const spawnMock = vi.fn(() => {
      queueMicrotask(() => {
        helper.emit('close', 0)
      })
      return helper
    })

    const service = new TaskService(spawnMock as never) as any
    const child = createMockChild({ pid: undefined, kill: vi.fn(() => false) })
    service.activeRuns.set('run-2', {
      child,
      run: { ...createTaskRun(), runId: 'run-2' },
      stoppedByUser: false,
      finished: false,
    })

    setTimeout(() => {
      ;(child as any).pid = 8765
      child.emit('spawn')
    }, 20)

    const result = await service.stopTask('run-2')

    expect(result).toEqual({ success: true })
    expect(spawnMock).toHaveBeenCalledWith('taskkill', ['/pid', '8765', '/t', '/f'], expect.any(Object))
  })
})
