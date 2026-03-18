import { ipcMain, dialog, BrowserWindow, shell } from 'electron'
import { IPC } from '../../shared/constants/index'
import { readDir, readFile, writeFile, createFile, pathExists, createDirectory, deletePath, renamePath, movePath } from '../services/fs'

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

  ipcMain.handle(IPC.FS_PATH_EXISTS, (_event, pathtoCheck: string) => pathExists(pathtoCheck))

  ipcMain.handle(IPC.FS_READ_DIR, (_event, dirPath: string) => readDir(dirPath))

  ipcMain.handle(IPC.FS_READ_FILE, (_event, filePath: string) => readFile(filePath))

  ipcMain.handle(IPC.FS_WRITE_FILE, (_event, filePath: string, content: string) =>
    writeFile(filePath, content)
  )

  ipcMain.handle(IPC.FS_CREATE_FILE, (_event, dirPath: string, fileName: string) =>
    createFile(dirPath, fileName)
  )

  ipcMain.handle(IPC.FS_CREATE_DIR, (_event, dirPath: string, folderName: string) =>
    createDirectory(dirPath, folderName)
  )

  ipcMain.handle(IPC.FS_DELETE_PATH, (_event, targetPath: string) =>
    deletePath(targetPath)
  )
  
  ipcMain.handle(IPC.FS_RENAME_PATH, (_event, oldPath: string, newPath: string) =>
    renamePath(oldPath, newPath)
  )

  ipcMain.handle(IPC.FS_MOVE_PATH, (_event, oldPath: string, newPath: string) =>
    movePath(oldPath, newPath)
  )

  ipcMain.on('shell:reveal', (_event, targetPath: string) => {
    shell.showItemInFolder(targetPath)
  })
}
