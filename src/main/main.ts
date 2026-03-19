import { app, BrowserWindow, protocol } from 'electron'
import si from 'systeminformation'
import path from 'path'
import { createWindow } from './services/window'
import { registerFileHandlers } from './ipc/file'
import { registerWorkspaceHandlers } from './ipc/workspace'
import { registerTerminalHandlers } from './ipc/terminal'
import { buildSysStats } from './services/systemMonitor'
import { workspaceFileIndex } from './services/fileIndex'
import { IPC } from '../shared/constants/index'

let mainWindow: BrowserWindow | null = null
const isDevelopment = process.env.NODE_ENV === 'development'
let stopWorkspaceChangeBridge: (() => void) | null = null

function isAllowedNavigation(url: string) {
  if (isDevelopment) {
    return url.startsWith('http://localhost:5173')
  }

  return url.startsWith('app://')
}

function startSysMonitor() {
  const poll = async () => {
    if (!mainWindow || mainWindow.isDestroyed()) return
    try {
      const load = await si.currentLoad()
      const mem = await si.mem()
      const graphics = await si.graphics()
      const disks = await si.fsSize()
      const processes = await si.processes()

      mainWindow.webContents.send('sys-stats', buildSysStats({
        load,
        mem,
        graphics,
        disks,
        processes,
      }))
    } catch { /* ignore */ }
    setTimeout(poll, 2000)
  }
  poll()
}

function bridgeWorkspaceChanges() {
  stopWorkspaceChangeBridge?.()
  stopWorkspaceChangeBridge = workspaceFileIndex.subscribe(() => {
    if (!mainWindow || mainWindow.isDestroyed()) return
    mainWindow.webContents.send(IPC.WORKSPACE_CHANGED, { timestamp: Date.now() })
  })
}

function getMimeType(filePath: string): string {
  const ext = path.extname(filePath).toLowerCase()
  const map: Record<string, string> = {
    '.html': 'text/html',
    '.js': 'application/javascript',
    '.css': 'text/css',
    '.json': 'application/json',
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.svg': 'image/svg+xml',
    '.ico': 'image/x-icon',
    '.woff': 'font/woff',
    '.woff2': 'font/woff2',
    '.ttf': 'font/ttf',
  }
  return map[ext] ?? 'application/octet-stream'
}

function resolveRendererAssetPath(requestPath: string): string {
  const rendererRoot = path.resolve(__dirname, '../../dist-renderer')
  const assetPath = path.resolve(rendererRoot, requestPath)
  const relativePath = path.relative(rendererRoot, assetPath)

  if (relativePath.startsWith('..') || path.isAbsolute(relativePath)) {
    throw new Error('Invalid renderer asset path')
  }

  return assetPath
}

// Register custom scheme to allow loading assets with "app://" privileges (fixes Vite module bugs)
protocol.registerSchemesAsPrivileged([
  { scheme: 'app', privileges: { secure: true, standard: true, bypassCSP: false, allowServiceWorkers: true, supportFetchAPI: true, corsEnabled: true } }
])

app.whenReady().then(() => {
  const fs = require('fs')

  // Setup custom protocol handler
  protocol.handle('app', async (request) => {
    try {
      const urlObject = new URL(request.url)
      let pathName = urlObject.pathname
      
      // If hostname is used as the root (e.g. app://index.html)
      if (urlObject.hostname && urlObject.hostname !== '.' && pathName === '/') {
        pathName = urlObject.hostname
      }

      if (pathName.startsWith('/')) pathName = pathName.slice(1)
      if (!pathName || pathName === '/') pathName = 'index.html'
      
      const filePath = resolveRendererAssetPath(pathName)
      
      if (fs.existsSync(filePath)) {
        const buffer = fs.readFileSync(filePath)
        return new Response(buffer, {
          headers: { 
            'Content-Type': getMimeType(filePath),
            'Access-Control-Allow-Origin': '*'
          }
        })
      }
      
      return new Response('File Not Found: ' + pathName, { status: 404 })
    } catch (err: any) {
      const logPath = path.join(app.getPath('userData'), 'protocol_error.txt')
      fs.appendFileSync(logPath, `Path: ${request.url}\nError: ${err.message}\n\n`)
      return new Response('Protocol Error', { status: 500 })
    }
  })

  registerFileHandlers()
  registerWorkspaceHandlers()
  registerTerminalHandlers()
  bridgeWorkspaceChanges()

  mainWindow = createWindow()
  startSysMonitor()
  mainWindow.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  mainWindow.webContents.on('will-navigate', (event, url) => {
    if (!isAllowedNavigation(url)) {
      event.preventDefault()
    }
  })

  // Logger for renderer console errors
  mainWindow.webContents.on('console-message', (event, level, message, line, sourceId) => {
    if (!isDevelopment) return
    const logPath = path.join(app.getPath('userData'), 'renderer_errors.txt')
    const logLine = `[Lvl:${level}] ${message} (at ${sourceId}:${line})\n`
    try { fs.appendFileSync(logPath, logLine) } catch {}
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    mainWindow = createWindow()
  }
})

app.on('before-quit', () => {
  stopWorkspaceChangeBridge?.()
  stopWorkspaceChangeBridge = null
})
