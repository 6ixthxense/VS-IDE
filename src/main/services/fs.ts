import fs from 'fs'
import path from 'path'
import type { FileEntry } from '../../shared/types/file'

export function readDir(dirPath: string): FileEntry[] {
  try {
    const entries = fs.readdirSync(dirPath, { withFileTypes: true })
    const result: FileEntry[] = []

    for (const e of entries) {
      if (
        e.name.startsWith('.') ||
        e.name === 'node_modules' ||
        e.name === 'dist' ||
        e.name === 'dist-electron' ||
        e.name === 'dist-renderer' ||
        e.name.startsWith('_temp_run')
      ) continue

      result.push({
        name: e.name,
        path: path.join(dirPath, e.name),
        isDirectory: e.isDirectory(),
      })
    }

    result.sort((a, b) => Number(b.isDirectory) - Number(a.isDirectory) || a.name.localeCompare(b.name))
    return result
  } catch {
    return []
  }
}

export function readFile(filePath: string): string {
  try {
    return fs.readFileSync(filePath, 'utf8')
  } catch {
    return ''
  }
}

export function writeFile(filePath: string, content: string): boolean {
  try {
    fs.writeFileSync(filePath, content, 'utf8')
    return true
  } catch {
    return false
  }
}

export function createFile(dirPath: string, fileName: string): { success: boolean; path?: string; error?: string } {
  try {
    const fullPath = path.join(dirPath, fileName)
    if (fs.existsSync(fullPath)) return { success: false, error: 'File already exists' }
    fs.writeFileSync(fullPath, '', 'utf8')
    return { success: true, path: fullPath }
  } catch (e: unknown) {
    return { success: false, error: (e as Error).message }
  }
}

export function createDirectory(dirPath: string, folderName: string): { success: boolean; path?: string; error?: string } {
  try {
    const fullPath = path.join(dirPath, folderName)
    if (fs.existsSync(fullPath)) return { success: false, error: 'Folder already exists' }
    fs.mkdirSync(fullPath)
    return { success: true, path: fullPath }
  } catch (e: unknown) {
    return { success: false, error: (e as Error).message }
  }
}

export function deletePath(targetPath: string): { success: boolean; error?: string } {
  try {
    if (!fs.existsSync(targetPath)) return { success: false, error: 'Path does not exist' }
    const stat = fs.statSync(targetPath)
    if (stat.isDirectory()) {
      fs.rmSync(targetPath, { recursive: true, force: true })
    } else {
      fs.unlinkSync(targetPath)
    }
    return { success: true }
  } catch (e: unknown) {
    return { success: false, error: (e as Error).message }
  }
}

export function renamePath(oldPath: string, newPath: string): { success: boolean; error?: string } {
  try {
    if (!fs.existsSync(oldPath)) return { success: false, error: 'Source path does not exist' }
    if (fs.existsSync(newPath)) return { success: false, error: 'Destination path already exists' }
    fs.renameSync(oldPath, newPath)
    return { success: true }
  } catch (e: unknown) {
    return { success: false, error: (e as Error).message }
  }
}

export function movePath(oldPath: string, newPath: string): { success: boolean; error?: string } {
  try {
    if (!fs.existsSync(oldPath)) return { success: false, error: 'Source path does not exist' }
    // If destination is a directory, move inside it
    const stat = fs.statSync(oldPath)
    let finalPath = newPath
    if (fs.existsSync(newPath) && fs.statSync(newPath).isDirectory()) {
       finalPath = path.join(newPath, path.basename(oldPath))
    }
    if (fs.existsSync(finalPath)) return { success: false, error: 'Destination already exists' }
    fs.renameSync(oldPath, finalPath)
    return { success: true }
  } catch (e: unknown) {
    return { success: false, error: (e as Error).message }
  }
}

export function pathExists(filePath: string): boolean {
  try {
    return fs.existsSync(filePath)
  } catch {
    return false
  }
}
