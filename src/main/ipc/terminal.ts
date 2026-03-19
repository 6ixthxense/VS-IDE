import { app, BrowserWindow, ipcMain } from 'electron'
import { ChildProcess, spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import { IPC } from '../../shared/constants/index'
import type { RunCodePayload, TerminalStatusEvent } from '../../shared/types/ipc'
import { diagnosticsService } from '../services/diagnostics'
import { TerminalService } from '../services/terminal'

// Track active processes and their input buffers for each terminal to route input correctly
const activeRuns = new Map<string, ChildProcess>()
const inputBuffers = new Map<string, string>()

function sendTerminalOutput(id: string, data: string) {
  BrowserWindow.getAllWindows().forEach((win) => {
    if (!win.isDestroyed()) {
      win.webContents.send(IPC.TERMINAL_OUT, { id, data })
    }
  })
}

function sendTerminalStatus(event: TerminalStatusEvent) {
  BrowserWindow.getAllWindows().forEach((win) => {
    if (!win.isDestroyed()) {
      win.webContents.send(IPC.TERMINAL_STATUS, event)
    }
  })
}

function cleanupTempFile(filePath: string) {
  void fs.promises.rm(filePath, { force: true }).catch(() => {})
}

export function registerTerminalHandlers() {
  // Initialize on register load or lazily
  TerminalService.getInstance().init()

  ipcMain.on(IPC.TERMINAL_INPUT, (_event, id: string, data: string) => {
    const activeRun = activeRuns.get(id)
    if (activeRun && activeRun.stdin && !activeRun.killed) {
      const buffer = inputBuffers.get(id) || ''
      
      if (data === '\r') {
        // Enter: Send current line buffer to process
        sendTerminalOutput(id, '\r\n')
        activeRun.stdin.write(buffer + '\n')
        inputBuffers.set(id, '')
      } else if (data === '\x7f' || data === '\x08') {
        // Backspace: Remove from local buffer only
        if (buffer.length > 0) {
          inputBuffers.set(id, buffer.slice(0, -1))
          sendTerminalOutput(id, '\b \b')
        }
      } else if (data === '\x03') {
        // Ctrl+C: Send interrupt and kill process
        inputBuffers.set(id, '')
        diagnosticsService.info('terminal', 'Run interrupted from terminal input.', `Terminal ${id}`)
        sendTerminalOutput(id, '^C\r\n')
        activeRun.kill('SIGINT')
      } else if (data === '\x1a' || data === '\x04') {
        // Ctrl+Z or Ctrl+D: Send remaining buffer then close stdin
        const char = data === '\x1a' ? '^Z' : '^D'
        if (buffer.length > 0) {
          activeRun.stdin.write(buffer)
          inputBuffers.set(id, '')
        }
        diagnosticsService.info('terminal', 'Run input stream closed from terminal input.', `Terminal ${id} via ${char}`)
        sendTerminalOutput(id, `${char}\r\n`)
        activeRun.stdin.end()
      } else {
        // Regular character: Add to local buffer and echo to UI
        // Note: We only echo when we buffer, so the user sees what they type
        inputBuffers.set(id, buffer + data)
        sendTerminalOutput(id, data)
      }
    } else {
      // Route input to the main PTY (PowerShell/Bash)
      TerminalService.getInstance().write(id, data)
    }
  })

  ipcMain.on(IPC.TERMINAL_RESIZE, (_event, id: string, cols: number, rows: number) => {
    TerminalService.getInstance().resize(id, cols, rows)
  })

  ipcMain.on(IPC.TERMINAL_SET_CWD, (_event, id: string, path: string) => {
    TerminalService.getInstance().setWorkingDirectory(id, path)
  })

  ipcMain.on(IPC.TERMINAL_CREATE, (_event, id: string, rootPath: string) => {
    TerminalService.getInstance().createTerminal(id, rootPath)
  })

  ipcMain.on(IPC.TERMINAL_CLOSE, (_event, id: string) => {
    TerminalService.getInstance().closeTerminal(id)
    // Kill any active run for this terminal
    const run = activeRuns.get(id)
    if (run) run.kill()
  })

  ipcMain.on(IPC.RUN_CODE, (_event, data: RunCodePayload) => {
    const terminalId = data.terminalId || 'default'
    const isPython = data.language === 'python'
    const ext = isPython ? '.py' : '.js'
    const cmd = isPython ? 'python' : 'node'
    const tempDir = path.join(app.getPath('temp'), 'vs-monitor-ide')
    const tmpFile = path.join(tempDir, `_temp_run_${Date.now()}${ext}`)

    // Kill existing run if any
    const existing = activeRuns.get(terminalId)
    if (existing) {
      existing.kill()
      inputBuffers.delete(terminalId)
    }

    try {
      if (!fs.existsSync(tempDir)) {
        fs.mkdirSync(tempDir, { recursive: true })
      }
      fs.writeFileSync(tmpFile, data.code)

      // 1. Send beautiful header
      sendTerminalOutput(terminalId, `\r\n\x1b[33m[Running] ${cmd} ${path.basename(tmpFile)}\x1b[0m\r\n`)

      // 2. Spawn process with piped input
      const args = isPython ? ['-u', tmpFile] : [tmpFile]
      const child = spawn(cmd, args, {
        cwd: path.dirname(tmpFile),
        env: { ...process.env, PYTHONIOENCODING: 'utf8' },
        shell: process.platform === 'win32' // Use shell on Windows to find command better
      })

      activeRuns.set(terminalId, child)

      child.stdout.on('data', (chunk) => {
        sendTerminalOutput(terminalId, chunk.toString().replace(/\n/g, '\r\n'))
      })

      child.stderr.on('data', (chunk) => {
        sendTerminalOutput(terminalId, `\x1b[31m${chunk.toString().replace(/\n/g, '\r\n')}\x1b[0m`)
      })

      child.on('close', (code) => {
        if (activeRuns.get(terminalId) === child) {
          activeRuns.delete(terminalId)
          inputBuffers.delete(terminalId)
        }
        sendTerminalOutput(terminalId, `\r\n\x1b[32m[Done] exited with code ${code}\x1b[0m\r\n`)
        if (typeof code === 'number' && code !== 0) {
          diagnosticsService.warn('terminal', 'Run finished with a non-zero exit code.', `${cmd} exited with code ${code}`)
          sendTerminalStatus({
            id: terminalId,
            level: 'warning',
            title: 'Run finished with errors',
            message: `${cmd} exited with code ${code}.`,
            details: `Temporary file: ${tmpFile}`,
          })
        }
        cleanupTempFile(tmpFile)
      })

      child.on('error', (err) => {
        if (activeRuns.get(terminalId) === child) {
          activeRuns.delete(terminalId)
        }
        diagnosticsService.error('terminal', 'Run failed to start.', err.message)
        sendTerminalOutput(terminalId, `\r\n\x1b[31m[Execute Error: ${err.message}]\x1b[0m\r\n`)
        sendTerminalStatus({
          id: terminalId,
          level: 'error',
          title: 'Run failed to start',
          message: `Unable to launch ${cmd}.`,
          details: err.message,
        })
        cleanupTempFile(tmpFile)
      })

    } catch (err: unknown) {
      diagnosticsService.error('terminal', 'Run preparation failed.', (err as Error).message)
      sendTerminalOutput(terminalId, `\r\n\x1b[31m[Run Error: ${(err as Error).message}]\x1b[0m\r\n`)
      sendTerminalStatus({
        id: terminalId,
        level: 'error',
        title: 'Run preparation failed',
        message: 'The file could not be prepared for execution.',
        details: (err as Error).message,
      })
    }
  })
}
