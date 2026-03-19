import * as pty from 'node-pty'
import { BrowserWindow } from 'electron'
import { IPC } from '../../shared/constants/index'

export class TerminalService {
  private static instance: TerminalService
  private ptyProcesses = new Map<string, pty.IPty>()
  private pendingCwds = new Map<string, string>()

  private constructor() {}

  public static getInstance(): TerminalService {
    if (!TerminalService.instance) {
      TerminalService.instance = new TerminalService()
    }
    return TerminalService.instance
  }

  public createTerminal(id: string, rootPath: string = process.env.HOME || process.env.USERPROFILE || 'C:\\') {
    if (this.ptyProcesses.has(id)) return

    const shell = process.platform === 'win32' ? 'powershell.exe' : 'bash'
    
    try {
      const ptyProcess = pty.spawn(shell, [], {
        name: 'xterm-color',
        cols: 80,
        rows: 24,
        cwd: rootPath,
        env: process.env as any
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

    } catch (err) {
      console.error(`Failed to spawn pty for ${id}:`, err)
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
