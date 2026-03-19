import { ipcMain } from 'electron'
import { IPC } from '../../shared/constants/index'
import path from 'path'
import { getGitDiff, getGitStatus, gitAdd, gitCommit, gitDiscard, gitPull, gitPush, gitUnstage } from '../services/git'
import { workspaceService } from '../services/workspace'
import { replaceInWorkspace, searchWorkspace } from '../services/search'
import type { GitDiffRequest, SearchOptions } from '@shared/types/ipc'
import { workspaceFileIndex } from '../services/fileIndex'

export function registerWorkspaceHandlers() {
  ipcMain.handle(IPC.WORKSPACE_GET, () => {
    return workspaceService.getRootPath() ?? path.join(__dirname, '../../..')
  })

  ipcMain.handle(IPC.WORKSPACE_SET, (_event, rootPath: string | null) => {
    const resolvedRootPath = workspaceService.setRootPath(rootPath)
    workspaceFileIndex.setRootPath(resolvedRootPath)
    return resolvedRootPath
  })

  ipcMain.handle('ws:search-files', async (_, query: string, _rootPath: string, options?: Partial<SearchOptions>) => {
    if (!query.trim()) return []

    try {
      return searchWorkspace(workspaceService.requireRootPath(), query, options)
    } catch {
      return []
    }
  })

  ipcMain.handle(IPC.GIT_STATUS, async () => {
    try {
      return getGitStatus(workspaceService.requireRootPath())
    } catch {
      return {}
    }
  })

  ipcMain.handle(
    'ws:replace-in-files',
    async (_,
      query: string,
      replacement: string,
      _rootPath: string,
      options?: Partial<SearchOptions>,
      targetPath?: string
    ) => {
    if (!query) return { success: false, error: 'Invalid parameters' }

    try {
      const rootPath = workspaceService.requireRootPath()
      const safeTargetPath = targetPath
        ? workspaceService.assertWithinWorkspace(targetPath, 'Target path')
        : undefined

      return replaceInWorkspace(rootPath, query, replacement, options, safeTargetPath)
    } catch (error) {
      return { success: false, count: 0, error: (error as Error).message }
    }
  })

  ipcMain.handle(IPC.GIT_ADD, async (_, _rootPath: string, filePaths: string[]) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      const safeFilePaths = filePaths.map((filePath) =>
        filePath === '.' ? '.' : workspaceService.toRelativeWorkspacePath(filePath)
      )

      return gitAdd(rootPath, safeFilePaths)
    } catch {
      return false
    }
  })

  ipcMain.handle(IPC.GIT_COMMIT, async (_, _rootPath: string, message: string) => {
    try {
      return gitCommit(workspaceService.requireRootPath(), message)
    } catch {
      return false
    }
  })

  ipcMain.handle(IPC.GIT_UNSTAGE, async (_, _rootPath: string, filePaths: string[]) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      const safeFilePaths = filePaths.map((filePath) => workspaceService.toRelativeWorkspacePath(filePath))
      return gitUnstage(rootPath, safeFilePaths)
    } catch {
      return false
    }
  })

  ipcMain.handle(IPC.GIT_DISCARD, async (_, _rootPath: string, request: GitDiffRequest) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      const safeFilePath = workspaceService.assertWithinWorkspace(request.filePath, 'Git discard path')
      return gitDiscard(rootPath, { ...request, filePath: safeFilePath })
    } catch {
      return false
    }
  })

  ipcMain.handle(IPC.GIT_PUSH, async () => {
    try {
      return gitPush(workspaceService.requireRootPath())
    } catch {
      return false
    }
  })

  ipcMain.handle(IPC.GIT_PULL, async () => {
    try {
      return gitPull(workspaceService.requireRootPath())
    } catch {
      return false
    }
  })

  ipcMain.handle('git:diff', async (_, _rootPath: string, request: GitDiffRequest) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      const safeFilePath = workspaceService.assertWithinWorkspace(request.filePath, 'Git diff path')
      return getGitDiff(rootPath, { ...request, filePath: safeFilePath })
    } catch {
      return 'Unable to load diff preview.'
    }
  })

  ipcMain.handle('ws:get-all-files', async () => {
    try {
      const rootPath = workspaceService.requireRootPath()
      return workspaceFileIndex.getFiles(rootPath)
    } catch {
      return []
    }
  })
}
