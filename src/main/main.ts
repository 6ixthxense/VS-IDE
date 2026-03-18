import { app, BrowserWindow, protocol, net, ipcMain } from 'electron'
import si from 'systeminformation'
import path from 'path'
import url from 'url'
import { createWindow } from './services/window'
import { registerFileHandlers } from './ipc/file'
import { registerWorkspaceHandlers } from './ipc/workspace'
import { registerTerminalHandlers } from './ipc/terminal'

let mainWindow: BrowserWindow | null = null

function startSysMonitor() {
  const poll = async () => {
    if (!mainWindow || mainWindow.isDestroyed()) return
    try {
      const load = await si.currentLoad()
      const mem = await si.mem()
      const gpu = await si.graphics()
      const disk = await si.fsSize()
      const processes = await si.processes()

      const topProcesses = processes.list
        .sort((a, b) => b.cpu - a.cpu)
        .slice(0, 5)
        .map(p => ({
          name: p.name,
          cpu: p.cpu.toFixed(1),
          mem: p.mem.toFixed(1),
          pid: p.pid
        }))

      const activeGpu = gpu.controllers.find((c: any) => 
        c.vendor?.toLowerCase().includes('nvidia') || 
        c.model?.toLowerCase().includes('rtx')
      ) || gpu.controllers[0];

      mainWindow.webContents.send('sys-stats', {
        cpu: load.currentLoad.toFixed(1),
        ram: (mem.used / mem.total * 100).toFixed(1),
        ramText: (mem.used / 1024 ** 3).toFixed(1) + ' / ' + (mem.total / 1024 ** 3).toFixed(1) + ' GB',
        gpuName: activeGpu?.model ?? 'N/A',
        gpu: activeGpu ? {
          load: activeGpu.utilizationGpu ?? 0,
          temp: activeGpu.temperatureGpu ?? 0,
          memTotal: activeGpu.memoryTotal ?? 0,
          memUsed: activeGpu.memoryUsed ?? 0
        } : null,
        disk: disk[0]
          ? { use: disk[0].use.toFixed(1), size: (disk[0].size / 1024 ** 3).toFixed(0), used: (disk[0].used / 1024 ** 3).toFixed(0) }
          : null,
        processes: topProcesses,
      })
    } catch { /* ignore */ }
    setTimeout(poll, 2000)
  }
  poll()
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
      
      const filePath = path.join(__dirname, '../../dist-renderer', pathName)
      
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

  mainWindow = createWindow()
  startSysMonitor()

  // FORCE DEVTOOLS IN PROD FOR DEBUGGING
  if (process.env.NODE_ENV !== 'development') {
    mainWindow.webContents.openDevTools({ mode: 'detach' })
  }

  // Logger for renderer console errors
  mainWindow.webContents.on('console-message', (event, level, message, line, sourceId) => {
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
