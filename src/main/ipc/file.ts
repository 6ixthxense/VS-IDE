import { ipcMain, dialog, BrowserWindow, shell } from 'electron'
import { IPC } from '../../shared/constants/index'
import { readDir, readFile, writeFile, createFile, pathExists, createDirectory, deletePath, renamePath, movePath } from '../services/fs'
import { workspaceService } from '../services/workspace'
import { workspaceFileIndex } from '../services/fileIndex'

export function registerFileHandlers() {
  ipcMain.handle(IPC.DIALOG_OPEN_FOLDER, async () => {
    const win = BrowserWindow.getFocusedWindow()
    if (!win) return null
    const result = await dialog.showOpenDialog(win, {
      properties: ['openDirectory'],
      title: 'Open Folder',
    })
    return result.canceled ? null : result.filePaths[0]
  })

  ipcMain.handle(IPC.FS_PATH_EXISTS, (_event, pathtoCheck: string) => {
    try {
      return pathExists(workspaceService.assertWithinWorkspace(pathtoCheck, 'Path'))
    } catch {
      return false
    }
  })

  ipcMain.handle(IPC.FS_READ_DIR, (_event, dirPath: string) => {
    try {
      return readDir(workspaceService.assertWithinWorkspace(dirPath, 'Directory path'))
    } catch {
      return []
    }
  })

  ipcMain.handle(IPC.FS_READ_FILE, (_event, filePath: string) => {
    try {
      return readFile(workspaceService.assertWithinWorkspace(filePath, 'File path'))
    } catch {
      return ''
    }
  })

  ipcMain.handle(IPC.FS_WRITE_FILE, (_event, filePath: string, content: string) => {
    try {
      return writeFile(workspaceService.assertWithinWorkspace(filePath, 'File path'), content)
    } catch {
      return false
    }
  })

  ipcMain.handle(IPC.FS_CREATE_FILE, (_event, dirPath: string, fileName: string) => {
    try {
      const result = createFile(
        workspaceService.assertWithinWorkspace(dirPath, 'Directory path'),
        workspaceService.assertWorkspaceEntryName(fileName, 'File name')
      )
      if (result.success && result.path) {
        workspaceFileIndex.addFile(result.path)
      }
      return result
    } catch (error) {
      return { success: false, error: (error as Error).message }
    }
  })

  ipcMain.handle(IPC.FS_CREATE_DIR, (_event, dirPath: string, folderName: string) => {
    try {
      return createDirectory(
        workspaceService.assertWithinWorkspace(dirPath, 'Directory path'),
        workspaceService.assertWorkspaceEntryName(folderName, 'Folder name')
      )
    } catch (error) {
      return { success: false, error: (error as Error).message }
    }
  })

  ipcMain.handle(IPC.FS_DELETE_PATH, (_event, targetPath: string) => {
    try {
      const safeTargetPath = workspaceService.assertWithinWorkspace(targetPath, 'Target path')
      const result = deletePath(safeTargetPath)
      if (result.success) {
        workspaceFileIndex.removePath(safeTargetPath)
      }
      return result
    } catch (error) {
      return { success: false, error: (error as Error).message }
    }
  })
  
  ipcMain.handle(IPC.FS_RENAME_PATH, (_event, oldPath: string, newPath: string) => {
    try {
      const safeOldPath = workspaceService.assertWithinWorkspace(oldPath, 'Source path')
      const safeNewPath = workspaceService.assertWithinWorkspace(newPath, 'Destination path')
      const result = renamePath(safeOldPath, safeNewPath)
      if (result.success && result.path) {
        workspaceFileIndex.renamePath(safeOldPath, result.path)
      }
      return result
    } catch (error) {
      return { success: false, error: (error as Error).message }
    }
  })

  ipcMain.handle(IPC.FS_MOVE_PATH, (_event, oldPath: string, newPath: string) => {
    try {
      const safeOldPath = workspaceService.assertWithinWorkspace(oldPath, 'Source path')
      const safeNewPath = workspaceService.assertWithinWorkspace(newPath, 'Destination path')
      const result = movePath(safeOldPath, safeNewPath)
      if (result.success && result.path) {
        workspaceFileIndex.renamePath(safeOldPath, result.path)
      }
      return result
    } catch (error) {
      return { success: false, error: (error as Error).message }
    }
  })

  ipcMain.on('shell:reveal', (_event, targetPath: string) => {
    try {
      shell.showItemInFolder(workspaceService.assertWithinWorkspace(targetPath, 'Target path'))
    } catch {
      // Ignore invalid reveal requests.
    }
  })
}
