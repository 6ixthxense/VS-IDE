import { ipcMain } from 'electron'
import { IPC } from '../../shared/constants'
import { problemsService } from '../services/problems'
import { workspaceService } from '../services/workspace'

export function registerProblemHandlers() {
  ipcMain.handle(IPC.PROBLEMS_SCAN, async () => {
    return problemsService.scan(workspaceService.requireRootPath())
  })

  ipcMain.handle(IPC.PROBLEMS_GET_LAST, async () => {
    return problemsService.getLastResult(workspaceService.requireRootPath())
  })
}
