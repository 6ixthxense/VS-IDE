const { app, BrowserWindow, ipcMain } = require('electron');
const { spawn } = require('child_process');
const path = require('path');
const si = require('systeminformation');
const fs = require('fs');

let mainWindow;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1400,
    height: 900,
    backgroundColor: '#1e1e1e',
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      nodeIntegration: false,
      contextIsolation: false
    }
  });
  mainWindow.loadFile('index.html');

  // System stats
  setInterval(async () => {
    if (!mainWindow || mainWindow.isDestroyed()) return;
    try {
      const [load, mem, gpu, disk] = await Promise.all([
        si.currentLoad(), si.mem(), si.graphics(), si.fsSize()
      ]);
      mainWindow.webContents.send('sys-stats', {
        cpu: load.currentLoad.toFixed(1),
        ram: (mem.used / mem.total * 100).toFixed(1),
        ramText: (mem.used / 1024 ** 3).toFixed(1) + ' / ' + (mem.total / 1024 ** 3).toFixed(1) + ' GB',
        gpuName: gpu.controllers[0] ? gpu.controllers[0].model : 'N/A',
        disk: disk[0] ? {
          use: disk[0].use.toFixed(1),
          size: (disk[0].size / 1024 ** 3).toFixed(0),
          used: (disk[0].used / 1024 ** 3).toFixed(0)
        } : null
      });
    } catch (e) { /* ignore */ }
  }, 2000);
}

// Run code
ipcMain.on('run-code', function(event, data) {
  var ext = data.language === 'python' ? '.py' : '.js';
  var cmd = data.language === 'python' ? 'python' : 'node';
  var tmpFile = path.join(__dirname, '_temp_run' + ext);
  fs.writeFileSync(tmpFile, data.code);
  var child = spawn(cmd, [tmpFile], { shell: true, cwd: __dirname });
  child.stdout.on('data', function(d) { event.sender.send('terminal-out', d.toString()); });
  child.stderr.on('data', function(d) { event.sender.send('terminal-out', '\x1b[31m' + d.toString() + '\x1b[0m'); });
  child.on('close', function(code) {
    event.sender.send('terminal-out', '\r\n\x1b[90m[exit code: ' + code + ']\x1b[0m\r\n');
    try { fs.unlinkSync(tmpFile); } catch (e) { /* ignore */ }
  });
});

// File system
ipcMain.handle('get-workspace', function() { return __dirname; });

ipcMain.handle('read-dir', function(_event, dirPath) {
  try {
    var entries = fs.readdirSync(dirPath, { withFileTypes: true });
    var result = [];
    for (var i = 0; i < entries.length; i++) {
      var e = entries[i];
      if (e.name.startsWith('.') || e.name === 'node_modules' || e.name === 'dist' || e.name.startsWith('_temp_run')) continue;
      result.push({ name: e.name, isDirectory: e.isDirectory(), path: path.join(dirPath, e.name) });
    }
    result.sort(function(a, b) { return (b.isDirectory - a.isDirectory) || a.name.localeCompare(b.name); });
    return result;
  } catch (e) { return []; }
});

ipcMain.handle('read-file', function(_event, filePath) {
  try { return fs.readFileSync(filePath, 'utf8'); }
  catch (e) { return ''; }
});

ipcMain.handle('save-file', function(_event, data) {
  try { fs.writeFileSync(data.filePath, data.content, 'utf8'); return true; }
  catch (e) { return false; }
});

ipcMain.handle('create-file', function(_event, data) {
  try {
    var fullPath = path.join(data.dirPath, data.fileName);
    if (fs.existsSync(fullPath)) return { success: false, error: 'File already exists' };
    fs.writeFileSync(fullPath, '', 'utf8');
    return { success: true, path: fullPath };
  } catch (e) { return { success: false, error: e.message }; }
});

app.whenReady().then(createWindow);
app.on('window-all-closed', function() { app.quit(); });
