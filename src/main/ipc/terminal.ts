import { app, BrowserWindow, ipcMain } from 'electron'
import { spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { IPC } from '../../shared/constants/index'
import type { RunCodePayload } from '../../shared/types/ipc'
import { TerminalService } from '../services/terminal'

function sendTerminalOutput(id: string, data: string) {
  BrowserWindow.getAllWindows().forEach((win) => {
    if (!win.isDestroyed()) {
      win.webContents.send(IPC.TERMINAL_OUT, { id, data })
    }
  })
}

function cleanupTempFile(filePath: string) {
  void fs.promises.rm(filePath, { force: true }).catch(() => {})
}

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

  ipcMain.on(IPC.RUN_CODE, (_event, data: RunCodePayload) => {
    const terminalId = data.terminalId || 'default'
    const isPython = data.language === 'python'
    const ext = isPython ? '.py' : '.js'
    const cmd = isPython ? 'python' : 'node'
    const tempDir = path.join(app.getPath('temp'), 'vs-monitor-ide')
    const tmpFile = path.join(tempDir, `_temp_run_${Date.now()}${ext}`)

    try {
      fs.mkdirSync(tempDir, { recursive: true })
      fs.writeFileSync(tmpFile, data.code)

      // 1. Send beautiful header
      sendTerminalOutput(terminalId, `\r\n\x1b[33m[Running] ${cmd} ${path.basename(tmpFile)}\x1b[0m\r\n`)

      // 2. Spawn process
      // Use unbuffered output for python if possible
      const args = isPython ? ['-u', tmpFile] : [tmpFile]
      const child = spawn(cmd, args, {
        cwd: path.dirname(tmpFile),
        env: { ...process.env, PYTHONIOENCODING: 'utf8' }
      })

      child.stdout.on('data', (chunk) => {
        sendTerminalOutput(terminalId, chunk.toString().replace(/\n/g, '\r\n'))
      })

      child.stderr.on('data', (chunk) => {
        sendTerminalOutput(terminalId, `\x1b[31m${chunk.toString().replace(/\n/g, '\r\n')}\x1b[0m`)
      })

      child.on('close', (code) => {
        sendTerminalOutput(terminalId, `\r\n\x1b[32m[Done] exited with code ${code}\x1b[0m\r\n`)
        cleanupTempFile(tmpFile)
      })

      child.on('error', (err) => {
        sendTerminalOutput(terminalId, `\r\n\x1b[31m[Execute Error: ${err.message}]\x1b[0m\r\n`)
        cleanupTempFile(tmpFile)
      })

    } catch (err: unknown) {
      cleanupTempFile(tmpFile)
      sendTerminalOutput(terminalId, `\r\n\x1b[31m[Run Error: ${(err as Error).message}]\x1b[0m\r\n`)
    }
  })
}
