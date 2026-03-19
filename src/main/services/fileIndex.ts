import fs from 'fs'
import path from 'path'

function shouldSkipDirectory(name: string) {
  return (
    name === 'node_modules' ||
    name === 'dist' ||
    name === 'dist-electron' ||
    name === 'dist-renderer' ||
    name === '.git' ||
    name.startsWith('.')
  )
}

function normalizePath(value: string) {
  const resolved = path.resolve(value)
  return process.platform === 'win32' ? resolved.toLowerCase() : resolved
}

function isWithinRoot(rootPath: string, targetPath: string) {
  const relativePath = path.relative(normalizePath(rootPath), normalizePath(targetPath))
  return relativePath === '' || (!relativePath.startsWith('..') && !path.isAbsolute(relativePath))
}

function shouldTrackPath(rootPath: string, targetPath: string) {
  if (!isWithinRoot(rootPath, targetPath)) return false

  const relativePath = path.relative(rootPath, targetPath)
  const segments = relativePath.split(path.sep).filter(Boolean)
  return !segments.some((segment) => shouldSkipDirectory(segment))
}

function sortFiles(filePaths: string[]) {
  return [...filePaths].sort((left, right) => left.localeCompare(right))
}

class WorkspaceFileIndexService {
  private rootPath: string | null = null
  private cachedFiles: string[] = []
  private buildPromise: Promise<string[]> | null = null
  private dirty = true
  private watcher: fs.FSWatcher | null = null
  private listeners = new Set<() => void>()

  public subscribe(listener: () => void) {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  public setRootPath(rootPath: string | null) {
    this.rootPath = rootPath ? path.resolve(rootPath) : null
    this.cachedFiles = []
    this.buildPromise = null
    this.dirty = true
    this.startWatcher()
  }

  public invalidate(targetPath?: string) {
    if (!this.rootPath) {
      this.dirty = true
      this.buildPromise = null
      return
    }

    if (!targetPath || isWithinRoot(this.rootPath, targetPath)) {
      this.dirty = true
      this.buildPromise = null
    }
  }

  public addFile(filePath: string) {
    if (!this.rootPath || !shouldTrackPath(this.rootPath, filePath)) return

    const resolvedPath = path.resolve(filePath)
    if (this.cachedFiles.includes(resolvedPath)) return

    this.cachedFiles = sortFiles([...this.cachedFiles, resolvedPath])
    this.notifyListeners()
  }

  public removePath(targetPath: string) {
    if (!this.rootPath || !isWithinRoot(this.rootPath, targetPath)) return

    const resolvedTargetPath = path.resolve(targetPath)
    const nextFiles = this.cachedFiles.filter((filePath) => {
      const relativePath = path.relative(normalizePath(resolvedTargetPath), normalizePath(filePath))
      return relativePath.startsWith('..') && !path.isAbsolute(relativePath)
    })

    if (nextFiles.length !== this.cachedFiles.length) {
      this.cachedFiles = nextFiles
      this.notifyListeners()
    }
  }

  public renamePath(oldPath: string, newPath: string) {
    if (!this.rootPath || !isWithinRoot(this.rootPath, oldPath)) {
      return
    }

    const resolvedOldPath = path.resolve(oldPath)
    const resolvedNewPath = path.resolve(newPath)
    const replaced = this.cachedFiles.map((filePath) => {
      const relativePath = path.relative(normalizePath(resolvedOldPath), normalizePath(filePath))
      if (relativePath === '') return resolvedNewPath
      if (!relativePath.startsWith('..') && !path.isAbsolute(relativePath)) {
        return path.join(resolvedNewPath, relativePath)
      }
      return filePath
    })

    const filtered = replaced.filter((filePath) => shouldTrackPath(this.rootPath!, filePath))
    const unique = Array.from(new Set(filtered))

    if (unique.length !== this.cachedFiles.length || unique.some((filePath, index) => filePath !== this.cachedFiles[index])) {
      this.cachedFiles = sortFiles(unique)
      this.notifyListeners()
    }
  }

  public notifyListeners() {
    this.listeners.forEach((listener) => listener())
  }

  public async getFiles(rootPath: string) {
    const resolvedRootPath = path.resolve(rootPath)

    if (this.rootPath !== resolvedRootPath) {
      this.setRootPath(resolvedRootPath)
    }

    if (!this.dirty) {
      return this.cachedFiles
    }

    if (!this.buildPromise) {
      this.buildPromise = this.buildIndex(resolvedRootPath)
        .then((files) => {
          this.cachedFiles = files
          this.dirty = false
          this.buildPromise = null
          return files
        })
        .catch((error) => {
          this.buildPromise = null
          throw error
        })
    }

    return this.buildPromise
  }

  private startWatcher() {
    this.watcher?.close()
    this.watcher = null

    if (!this.rootPath) return

    try {
      this.watcher = fs.watch(this.rootPath, { recursive: true }, (eventType, filename) => {
        void this.handleWatchEvent(eventType, filename)
      })
    } catch {
      this.invalidate()
    }
  }

  private async handleWatchEvent(eventType: string, filename: string | Buffer | null) {
    if (!this.rootPath) return

    if (!filename) {
      this.invalidate()
      this.notifyListeners()
      return
    }

    const nextPath = path.join(this.rootPath, filename.toString())
    if (!shouldTrackPath(this.rootPath, nextPath)) return

    try {
      const stats = await fs.promises.stat(nextPath)

      if (stats.isDirectory()) {
        this.invalidate(nextPath)
        this.notifyListeners()
        return
      }

      if (eventType === 'rename') {
        this.addFile(nextPath)
      } else {
        this.notifyListeners()
      }
    } catch {
      this.removePath(nextPath)
    }
  }

  private async buildIndex(rootPath: string) {
    const files: string[] = []

    const scan = async (dirPath: string): Promise<void> => {
      const entries = await fs.promises.readdir(dirPath, { withFileTypes: true })

      for (const entry of entries) {
        const fullPath = path.join(dirPath, entry.name)

        if (entry.isDirectory()) {
          if (shouldSkipDirectory(entry.name)) continue
          await scan(fullPath)
          continue
        }

        if (entry.isFile()) {
          files.push(fullPath)
        }
      }
    }

    await scan(rootPath)
    return sortFiles(files)
  }
}

export const workspaceFileIndex = new WorkspaceFileIndexService()
