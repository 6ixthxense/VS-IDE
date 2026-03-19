import fs from 'fs'
import * as pty from 'node-pty'
import { BrowserWindow } from 'electron'
import { IPC } from '../../shared/constants/index'
import type { OperationResult, TerminalStatusEvent } from '../../shared/types/ipc'
import { diagnosticsService } from './diagnostics'

export class TerminalService {
  private static instance: TerminalService
  private ptyProcesses = new Map<string, pty.IPty>()
  private pendingCwds = new Map<string, string>()

  private constructor() {}

  private broadcastStatus(event: TerminalStatusEvent) {
    const wins = BrowserWindow.getAllWindows()
    wins.forEach((win) => {
      if (!win.isDestroyed()) {
        win.webContents.send(IPC.TERMINAL_STATUS, event)
      }
    })
  }

  public static getInstance(): TerminalService {
    if (!TerminalService.instance) {
      TerminalService.instance = new TerminalService()
    }
    return TerminalService.instance
  }

  public createTerminal(id: string, rootPath: string = process.env.HOME || process.env.USERPROFILE || 'C:\\'): OperationResult {
    if (this.ptyProcesses.has(id)) return { success: true }

    const shell = process.platform === 'win32' ? 'powershell.exe' : 'bash'
    const cwd = rootPath && fs.existsSync(rootPath)
      ? rootPath
      : (process.env.HOME || process.env.USERPROFILE || 'C:\\')
    
    try {
      const ptyProcess = pty.spawn(shell, [], {
        name: 'xterm-color',
        cols: 80,
        rows: 24,
        cwd,
        env: process.env as any, useConpty: false
      })

      ptyProcess.onData((data) => {
        const wins = BrowserWindow.getAllWindows()
        wins.forEach(win => {
          if (!win.isDestroyed()) {
            win.webContents.send(IPC.TERMINAL_OUT, { id, data })
          }
        })
      })

      this.ptyProcesses.set(id, ptyProcess)

      setTimeout(() => {
        ptyProcess.write('\r')
        const pending = this.pendingCwds.get(id)
        if (pending) {
          this.setWorkingDirectory(id, pending)
          this.pendingCwds.delete(id)
        }
      }, 800)

      return { success: true }

    } catch (err) {
      diagnosticsService.error('terminal', 'Integrated terminal launch failed.', err instanceof Error ? err.message : String(err))
      this.broadcastStatus({
        id,
        level: 'error',
        title: 'Terminal launch failed',
        message: 'The integrated terminal could not be created.',
        details: err instanceof Error ? err.message : String(err),
      })
      return {
        success: false,
        error: 'The integrated terminal could not be created',
        details: err instanceof Error ? err.message : String(err),
      }
    }
  }

  public init(rootPath: string = process.env.HOME || process.env.USERPROFILE || 'C:\\') {
    this.createTerminal('default', rootPath)
  }

  public write(id: string, data: string) {
    const ptyProcess = this.ptyProcesses.get(id)
    if (ptyProcess) {
      ptyProcess.write(data)
    }
  }

  public setWorkingDirectory(id: string, newPath: string) {
    const ptyProcess = this.ptyProcesses.get(id)
    if (ptyProcess) {
      const cdCmd = `cd "${newPath}"\r`
      ptyProcess.write(cdCmd)
    } else {
      this.pendingCwds.set(id, newPath)
    }
  }

  public resize(id: string, cols: number, rows: number) {
    const ptyProcess = this.ptyProcesses.get(id)
    if (ptyProcess) {
      try {
        ptyProcess.resize(cols, rows)
      } catch {
        /* ignore */
      }
    }
  }

  public closeTerminal(id: string) {
    const ptyProcess = this.ptyProcesses.get(id)
    if (ptyProcess) {
      ptyProcess.kill()
      this.ptyProcesses.delete(id)
    }
  }
}
