import { ipcMain } from 'electron'
import { spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { IPC } from '../../shared/constants/index'
import type { RunCodePayload } from '../../shared/types/ipc'
import { TerminalService } from '../services/terminal'

export function registerTerminalHandlers() {
  // Initialize on register load or lazily
  TerminalService.getInstance().init()

  ipcMain.on('terminal:input', (_event, id: string, data: string) => {
    TerminalService.getInstance().write(id, data)
  })

  ipcMain.on('terminal:resize', (_event, id: string, cols: number, rows: number) => {
    TerminalService.getInstance().resize(id, cols, rows)
  })

  ipcMain.on('terminal:set-cwd', (_event, id: string, path: string) => {
    TerminalService.getInstance().setWorkingDirectory(id, path)
  })

  ipcMain.on('terminal:create', (_event, id: string, rootPath: string) => {
    TerminalService.getInstance().createTerminal(id, rootPath)
  })

  ipcMain.on('terminal:close', (_event, id: string) => {
    TerminalService.getInstance().closeTerminal(id)
  })

  ipcMain.on(IPC.RUN_CODE, (event, data: RunCodePayload) => {
    const isPython = data.language === 'python'
    const ext = isPython ? '.py' : '.js'
    const cmd = isPython ? 'python' : 'node'
    const tmpFile = path.join(__dirname, '_temp_run' + ext)

    try {
      fs.writeFileSync(tmpFile, data.code)
      
      const wins = require('electron').BrowserWindow.getAllWindows()
      const sendToTerm = (msg: string) => {
        wins.forEach((win: any) => {
          if (!win.isDestroyed()) {
            win.webContents.send(IPC.TERMINAL_OUT, msg)
          }
        })
      }

      // 1. Send beautiful header
      sendToTerm(`\r\n\x1b[33m[Running] ${cmd} ${path.basename(tmpFile)}\x1b[0m\r\n`)

      // 2. Spawn process
      // Use unbuffered output for python if possible
      const args = isPython ? ['-u', tmpFile] : [tmpFile]
      const child = spawn(cmd, args, {
        cwd: path.dirname(tmpFile),
        env: { ...process.env, PYTHONIOENCODING: 'utf8' }
      })

      child.stdout.on('data', (data) => {
        sendToTerm(data.toString().replace(/\n/g, '\r\n'))
      })

      child.stderr.on('data', (data) => {
        sendToTerm(`\x1b[31m${data.toString().replace(/\n/g, '\r\n')}\x1b[0m`)
      })

      child.on('close', (code) => {
        sendToTerm(`\r\n\x1b[32m[Done] exited with code ${code}\x1b[0m\r\n`)
      })

      child.on('error', (err) => {
        sendToTerm(`\r\n\x1b[31m[Execute Error: ${err.message}]\x1b[0m\r\n`)
      })

    } catch (err: unknown) {
      const wins = require('electron').BrowserWindow.getAllWindows()
      wins.forEach((win: any) => {
        if (!win.isDestroyed()) {
          win.webContents.send(IPC.TERMINAL_OUT, `\r\n\x1b[31m[Run Error: ${(err as Error).message}]\x1b[0m\r\n`)
        }
      })
    }
  })
}
