import { ipcMain } from 'electron'
import { IPC } from '../../shared/constants/index'
import path from 'path'
import { getGitDiff, getGitStatus, gitAdd, gitCommit, gitDiscard, gitPull, gitPush, gitUnstage } from '../services/git'
import { workspaceService } from '../services/workspace'
import { replaceInWorkspace, searchWorkspace } from '../services/search'
import type { GitDiffRequest, OperationResult, SearchOptions } from '@shared/types/ipc'
import { workspaceFileIndex } from '../services/fileIndex'

function failedOperation(error: unknown, fallback: string): OperationResult {
  return {
    success: false,
    error: fallback,
    details: error instanceof Error ? error.message : String(error),
  }
}

function resolveGitRequestPath(request: GitDiffRequest, label: string) {
  const legacyRequest = request as GitDiffRequest & { path?: string }
  return workspaceService.assertWithinWorkspace(request.filePath ?? legacyRequest.path, label)
}

export function registerWorkspaceHandlers() {
  ipcMain.handle(IPC.WORKSPACE_GET, () => {
    return workspaceService.getRootPath() ?? path.join(__dirname, '../../..')
  })

  ipcMain.handle(IPC.WORKSPACE_SET, (_event, rootPath: string | null) => {
    const resolvedRootPath = workspaceService.setRootPath(rootPath)
    workspaceFileIndex.setRootPath(resolvedRootPath)
    return resolvedRootPath
  })

  ipcMain.handle(IPC.WORKSPACE_SEARCH_FILES, async (_, query: string, _rootPath: string, options?: Partial<SearchOptions>) => {
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
    IPC.WORKSPACE_REPLACE_IN_FILES,
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
    } catch (error) {
      return failedOperation(error, 'Unable to stage the selected changes')
    }
  })

  ipcMain.handle(IPC.GIT_COMMIT, async (_, _rootPath: string, message: string) => {
    try {
      return gitCommit(workspaceService.requireRootPath(), message)
    } catch (error) {
      return failedOperation(error, 'Commit failed')
    }
  })

  ipcMain.handle(IPC.GIT_UNSTAGE, async (_, _rootPath: string, filePaths: string[]) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      const safeFilePaths = filePaths.map((filePath) => workspaceService.toRelativeWorkspacePath(filePath))
      return gitUnstage(rootPath, safeFilePaths)
    } catch (error) {
      return failedOperation(error, 'Unable to unstage the selected changes')
    }
  })

  ipcMain.handle(IPC.GIT_DISCARD, async (_, _rootPath: string, request: GitDiffRequest) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      const safeFilePath = resolveGitRequestPath(request, 'Git discard path')
      return gitDiscard(rootPath, { ...request, filePath: safeFilePath })
    } catch (error) {
      return failedOperation(error, 'Unable to discard local changes')
    }
  })

  ipcMain.handle(IPC.GIT_PUSH, async () => {
    try {
      return gitPush(workspaceService.requireRootPath())
    } catch (error) {
      return failedOperation(error, 'Push failed')
    }
  })

  ipcMain.handle(IPC.GIT_PULL, async () => {
    try {
      return gitPull(workspaceService.requireRootPath())
    } catch (error) {
      return failedOperation(error, 'Pull failed')
    }
  })

  ipcMain.handle(IPC.GIT_DIFF, async (_, _rootPath: string, request: GitDiffRequest) => {
    try {
      const rootPath = workspaceService.requireRootPath()
      const safeFilePath = resolveGitRequestPath(request, 'Git diff path')
      return getGitDiff(rootPath, { ...request, filePath: safeFilePath })
    } catch {
      return 'Unable to load diff preview.'
    }
  })

  ipcMain.handle(IPC.WORKSPACE_GET_ALL_FILES, async () => {
    try {
      const rootPath = workspaceService.requireRootPath()
      return workspaceFileIndex.getFiles(rootPath)
    } catch {
      return []
    }
  })
}
