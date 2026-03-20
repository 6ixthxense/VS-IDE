import fs from 'fs'
import os from 'os'
import path from 'path'
import { describe, expect, it } from 'vitest'
import { discoverPackageScriptTasks } from '../../main/services/tasks'

function createWorkspaceWithPackageJson(scripts: Record<string, string>) {
  const rootPath = fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-tasks-'))
  fs.writeFileSync(
    path.join(rootPath, 'package.json'),
    JSON.stringify({ name: 'sample', scripts }, null, 2),
    'utf8'
  )
  return rootPath
}

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
})
