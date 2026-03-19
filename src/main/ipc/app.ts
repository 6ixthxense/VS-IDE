import { ipcMain } from 'electron'
import { IPC } from '../../shared/constants'
import { diagnosticsService } from '../services/diagnostics'
import { updateService } from '../services/updater'

export function registerAppHandlers() {
  ipcMain.handle(IPC.APP_INFO, () => updateService.getAppInfo())
  ipcMain.handle(IPC.APP_GET_DIAGNOSTICS, () => diagnosticsService.getSnapshot())
  ipcMain.handle(IPC.APP_CLEAR_DIAGNOSTICS, () => diagnosticsService.clear())
  ipcMain.handle(IPC.APP_OPEN_LOG_FOLDER, () => diagnosticsService.openLogFolder())
  ipcMain.handle(IPC.APP_CHECK_FOR_UPDATES, () => updateService.checkForUpdates())
  ipcMain.handle(IPC.APP_INSTALL_UPDATE, () => updateService.installUpdate())
}
