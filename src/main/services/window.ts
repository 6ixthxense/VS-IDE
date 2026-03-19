import { BrowserWindow } from 'electron'
import path from 'path'

export function createWindow(): BrowserWindow {
  const isDevelopment = process.env.NODE_ENV === 'development'
  const windowIconPath = path.resolve(process.cwd(), 'build', 'icon.png')

  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    minWidth: 900,
    minHeight: 600,
    backgroundColor: '#1e1e1e',
    autoHideMenuBar: true,
    frame: true,
    icon: windowIconPath,
    webPreferences: {
      preload: path.join(__dirname, '../../preload/preload.js'),
      nodeIntegration: false,
      contextIsolation: true,
      // The preload now imports local helper modules, which requires an unsandboxed preload.
      sandbox: false,
      devTools: isDevelopment,
    },
  })

  if (isDevelopment) {
    win.loadURL('http://localhost:5173')
    // win.webContents.openDevTools({ mode: 'detach' })
  } else {
    win.loadURL('app://index.html')
  }

  return win
}
