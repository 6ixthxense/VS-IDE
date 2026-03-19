import fs from 'fs'
import path from 'path'

function normalizeForComparison(targetPath: string): string {
  const resolvedPath = path.resolve(targetPath)
  return process.platform === 'win32' ? resolvedPath.toLowerCase() : resolvedPath
}

function isWithinPath(parentPath: string, targetPath: string): boolean {
  const relativePath = path.relative(
    normalizeForComparison(parentPath),
    normalizeForComparison(targetPath)
  )

  return relativePath === '' || (!relativePath.startsWith('..') && !path.isAbsolute(relativePath))
}

class WorkspaceService {
  private rootPath: string | null = null

  public setRootPath(rootPath: string | null): string | null {
    if (!rootPath) {
      this.rootPath = null
      return null
    }

    const resolvedRootPath = path.resolve(rootPath)
    if (!fs.existsSync(resolvedRootPath)) {
      throw new Error('Workspace path does not exist')
    }

    if (!fs.statSync(resolvedRootPath).isDirectory()) {
      throw new Error('Workspace path must be a directory')
    }

    this.rootPath = resolvedRootPath
    return resolvedRootPath
  }

  public getRootPath(): string | null {
    return this.rootPath
  }

  public requireRootPath(): string {
    if (!this.rootPath) {
      throw new Error('No workspace folder is open')
    }

    return this.rootPath
  }

  public assertWithinWorkspace(targetPath: string, label = 'Path'): string {
    const rootPath = this.requireRootPath()
    const resolvedTargetPath = path.resolve(targetPath)

    if (!isWithinPath(rootPath, resolvedTargetPath)) {
      throw new Error(`${label} must stay inside the open workspace`)
    }

    return resolvedTargetPath
  }

  public assertWorkspaceEntryName(name: string, label = 'Name'): string {
    const trimmedName = name.trim()

    if (!trimmedName || trimmedName === '.' || trimmedName === '..') {
      throw new Error(`${label} is invalid`)
    }

    if (path.basename(trimmedName) !== trimmedName) {
      throw new Error(`${label} must not contain path separators`)
    }

    return trimmedName
  }

  public toRelativeWorkspacePath(targetPath: string): string {
    const rootPath = this.requireRootPath()
    const resolvedTargetPath = this.assertWithinWorkspace(targetPath)
    const relativePath = path.relative(rootPath, resolvedTargetPath)

    return relativePath || '.'
  }
}

export const workspaceService = new WorkspaceService()
