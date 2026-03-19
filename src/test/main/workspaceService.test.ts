import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { workspaceService } from '../../main/services/workspace'

describe('workspaceService', () => {
  let rootPath = ''

  beforeEach(() => {
    rootPath = fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-ide-workspace-'))
    workspaceService.setRootPath(rootPath)
  })

  afterEach(() => {
    workspaceService.setRootPath(null)
    fs.rmSync(rootPath, { recursive: true, force: true })
  })

  it('allows nested paths inside the active workspace and returns a relative git-safe path', () => {
    const nestedFilePath = path.join(rootPath, 'src', 'main.ts')

    expect(workspaceService.assertWithinWorkspace(nestedFilePath)).toBe(path.resolve(nestedFilePath))
    expect(workspaceService.toRelativeWorkspacePath(nestedFilePath)).toBe(path.join('src', 'main.ts'))
  })

  it('rejects paths that escape the active workspace', () => {
    const outsidePath = path.resolve(rootPath, '..', 'outside.ts')

    expect(() => workspaceService.assertWithinWorkspace(outsidePath, 'Target path')).toThrow(
      'Target path must stay inside the open workspace'
    )
  })

  it('rejects invalid entry names that try to smuggle path traversal', () => {
    expect(() => workspaceService.assertWorkspaceEntryName('../escape.ts', 'File name')).toThrow(
      'File name must not contain path separators'
    )
    expect(() => workspaceService.assertWorkspaceEntryName('   ', 'File name')).toThrow(
      'File name is invalid'
    )
  })
})
