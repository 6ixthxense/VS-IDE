import { app, BrowserWindow } from 'electron'
import { autoUpdater } from 'electron-updater'
import fs from 'fs'
import path from 'path'
import { IPC } from '../../shared/constants'
import type { AppInfo, OperationResult, UpdateStatusEvent } from '../../shared/types/ipc'
import { diagnosticsService } from './diagnostics'

interface BundledUpdateConfig {
  updateUrl?: string
  channel?: string
}

function toOperationFailure(error: unknown, fallback: string): OperationResult {
  return {
    success: false,
    error: fallback,
    details: error instanceof Error ? error.message : String(error),
  }
}

export class UpdateService {
  private configured = false
  private listenersRegistered = false
  private downloadedUpdateReady = false
  private updateSource = ''
  private updateChannel = 'latest'
  private lastStatus: UpdateStatusEvent = {
    state: 'idle',
    message: 'Automatic updates are idle.',
  }

  private getBundledConfig(): BundledUpdateConfig | null {
    if (!app.isPackaged) return null

    const configPath = path.join(process.resourcesPath, 'update-config.json')
    if (!fs.existsSync(configPath)) return null

    try {
      const raw = fs.readFileSync(configPath, 'utf8')
      return JSON.parse(raw) as BundledUpdateConfig
    } catch (error) {
      diagnosticsService.warn('updater', 'Bundled update config could not be parsed.', error instanceof Error ? error.message : String(error))
      return null
    }
  }

  private resolveFeedConfig() {
    const envUrl = process.env.VSMONITOR_UPDATE_URL || process.env.VS_MONITOR_UPDATE_URL || ''
    const bundledConfig = this.getBundledConfig()
    const updateUrl = envUrl || bundledConfig?.updateUrl || ''
    const channel = process.env.VSMONITOR_UPDATE_CHANNEL || bundledConfig?.channel || 'latest'
    const source = envUrl ? 'environment override' : updateUrl ? 'bundled release config' : ''

    return { updateUrl, channel, source }
  }

  private broadcast(status: UpdateStatusEvent) {
    this.lastStatus = status

    BrowserWindow.getAllWindows().forEach((win) => {
      if (!win.isDestroyed()) {
        win.webContents.send(IPC.APP_UPDATE_STATUS, status)
      }
    })
  }

  public configure() {
    const feedConfig = this.resolveFeedConfig()
    this.updateSource = feedConfig.source
    this.updateChannel = feedConfig.channel
    this.configured = app.isPackaged && Boolean(feedConfig.updateUrl)

    if (!this.configured) {
      diagnosticsService.info('updater', 'Automatic updates are not configured.', app.isPackaged ? 'Package without update-config.json or VSMONITOR_UPDATE_URL.' : 'Development build.')
      this.broadcast({
        state: 'idle',
        message: app.isPackaged
          ? 'Automatic updates are disabled for this build.'
          : 'Automatic updates are available in packaged builds.',
      })
      return
    }

    if (!this.listenersRegistered) {
      autoUpdater.autoDownload = true
      autoUpdater.autoInstallOnAppQuit = true
      autoUpdater.setFeedURL({
        provider: 'generic',
        url: feedConfig.updateUrl,
        channel: feedConfig.channel,
      })

      autoUpdater.on('checking-for-update', () => {
        diagnosticsService.info('updater', 'Checking for updates.')
        this.broadcast({ state: 'checking', message: 'Checking for updates...' })
      })

      autoUpdater.on('update-available', (info) => {
        this.downloadedUpdateReady = false
        diagnosticsService.info('updater', 'Update available.', `Version ${info.version}`)
        this.broadcast({
          state: 'available',
          message: `Version ${info.version} is available and downloading now.`,
          version: info.version,
        })
      })

      autoUpdater.on('update-not-available', (info) => {
        this.downloadedUpdateReady = false
        diagnosticsService.info('updater', 'No update available.', `Current version ${info.version}`)
        this.broadcast({
          state: 'not-available',
          message: 'You already have the latest version.',
          version: info.version,
        })
      })

      autoUpdater.on('download-progress', (progress) => {
        diagnosticsService.info('updater', 'Update download progress.', `${Math.round(progress.percent)}%`)
        this.broadcast({
          state: 'downloading',
          message: `Downloading update... ${Math.round(progress.percent)}%`,
          progress: progress.percent,
        })
      })

      autoUpdater.on('update-downloaded', (info) => {
        this.downloadedUpdateReady = true
        diagnosticsService.info('updater', 'Update downloaded.', `Version ${info.version}`)
        this.broadcast({
          state: 'downloaded',
          message: `Version ${info.version} is ready to install.`,
          version: info.version,
        })
      })

      autoUpdater.on('error', (error) => {
        this.downloadedUpdateReady = false
        diagnosticsService.error('updater', 'Automatic update failed.', error == null ? 'Unknown updater error' : String(error))
        this.broadcast({
          state: 'error',
          message: 'Automatic update failed.',
          details: error == null ? 'Unknown updater error' : String(error),
        })
      })

      this.listenersRegistered = true
    }

    this.broadcast({
      state: 'idle',
      message: 'Automatic updates are ready.',
    })
    diagnosticsService.info('updater', 'Automatic updates configured.', `${feedConfig.updateUrl} [${feedConfig.channel}]`)
  }

  public getAppInfo(): AppInfo {
    return {
      version: app.getVersion(),
      isPackaged: app.isPackaged,
      updateConfigured: this.configured,
      bridgeAvailable: true,
      updateChannel: this.updateChannel,
      updateSource: this.updateSource,
    }
  }

  public getLastStatus() {
    return this.lastStatus
  }

  public async checkForUpdates(): Promise<OperationResult> {
    if (!this.configured) {
      return {
        success: false,
        error: 'Automatic updates are not configured',
        details: 'Build a packaged app with build/update-config.json or set VSMONITOR_UPDATE_URL at runtime to enable update checks.',
      }
    }

    try {
      await autoUpdater.checkForUpdates()
      return { success: true }
    } catch (error) {
      return toOperationFailure(error, 'Unable to check for updates')
    }
  }

  public installUpdate(): OperationResult {
    if (!this.downloadedUpdateReady) {
      return {
        success: false,
        error: 'No downloaded update is ready yet',
        details: 'Check for updates and wait for the download to finish first.',
      }
    }

    setImmediate(() => {
      autoUpdater.quitAndInstall()
    })

    return { success: true }
  }
}

export const updateService = new UpdateService()
