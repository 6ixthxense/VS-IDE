import { ipcMain } from 'electron'
import { IPC } from '../../shared/constants/index'
import path from 'path'
import fs from 'fs'
import { getGitStatus, gitAdd, gitCommit, gitPush, gitPull } from '../services/git'

export function registerWorkspaceHandlers() {
  ipcMain.handle(IPC.WORKSPACE_GET, () => {
    // Default workspace = project root (two levels up from dist-electron/main)
    return path.join(__dirname, '../../..')
  })

  ipcMain.handle('ws:search-files', async (_, query: string, rootPath: string) => {
    if (!rootPath || !query.trim()) return []
    const results: { path: string; name: string; line: number; match: string }[] = []

    async function searchDir(dir: string) {
      try {
        const items = await fs.promises.readdir(dir, { withFileTypes: true })
        for (const item of items) {
          const resPath = path.join(dir, item.name)
          if (item.isDirectory()) {
            if (item.name === 'node_modules' || item.name === 'dist' || item.name.startsWith('.')) continue
            await searchDir(resPath)
          } else if (item.isFile()) {
            try {
              const content = await fs.promises.readFile(resPath, 'utf-8')
              if (content.toLowerCase().includes(query.toLowerCase())) {
                const lines = content.split('\n')
                for (let i = 0; i < lines.length; i++) {
                  if (lines[i].toLowerCase().includes(query.toLowerCase())) {
                    results.push({
                      path: resPath,
                      name: item.name,
                      line: i + 1,
                      match: lines[i].trim(),
                    })
                  }
                  if (results.length > 200) break // Cap results
                }
              }
            } catch {}
          }
        }
      } catch {}
    }

    await searchDir(rootPath)
    return results
  })

  ipcMain.handle(IPC.GIT_STATUS, async (_, rootPath: string) => {
    return getGitStatus(rootPath)
  })

  ipcMain.handle('ws:replace-in-files', async (_, query: string, replacement: string, rootPath: string, targetPath?: string) => {
    if (!rootPath || !query) return { success: false, error: 'Invalid parameters' }
    
    let count = 0
    async function processDir(dir: string) {
      try {
        const items = await fs.promises.readdir(dir, { withFileTypes: true })
        for (const item of items) {
          const resPath = path.join(dir, item.name)
          if (targetPath && !resPath.startsWith(targetPath)) continue
          
          if (item.isDirectory()) {
            if (item.name === 'node_modules' || item.name === 'dist' || item.name.startsWith('.')) continue
            await processDir(resPath)
          } else if (item.isFile()) {
            try {
              const content = await fs.promises.readFile(resPath, 'utf-8')
              if (content.includes(query)) {
                const newContent = content.split(query).join(replacement)
                await fs.promises.writeFile(resPath, newContent, 'utf-8')
                count++
              }
            } catch {}
          }
        }
      } catch {}
    }

    if (targetPath) {
       // Single file or specific dir
       const stats = await fs.promises.stat(targetPath)
       if (stats.isFile()) {
         const content = await fs.promises.readFile(targetPath, 'utf-8')
         if (content.includes(query)) {
           const newContent = content.split(query).join(replacement)
           await fs.promises.writeFile(targetPath, newContent, 'utf-8')
           count = 1
         }
       } else {
         await processDir(targetPath)
       }
    } else {
      await processDir(rootPath)
    }
    
    return { success: true, count }
  })

  ipcMain.handle(IPC.GIT_ADD, async (_, rootPath: string, filePaths: string[]) => {
    return gitAdd(rootPath, filePaths)
  })

  ipcMain.handle(IPC.GIT_COMMIT, async (_, rootPath: string, message: string) => {
    return gitCommit(rootPath, message)
  })

  ipcMain.handle(IPC.GIT_PUSH, async (_, rootPath: string) => {
    return gitPush(rootPath)
  })

  ipcMain.handle(IPC.GIT_PULL, async (_, rootPath: string) => {
    return gitPull(rootPath)
  })

  ipcMain.handle('ws:get-all-files', async (_, rootPath: string) => {
    if (!rootPath) return []
    const files: string[] = []
    
    async function scan(dir: string) {
      try {
        const entries = await fs.promises.readdir(dir, { withFileTypes: true })
        for (const entry of entries) {
          const fullPath = path.join(dir, entry.name)
          if (entry.isDirectory()) {
            if (entry.name === 'node_modules' || entry.name === '.git' || entry.name === 'dist') continue
            await scan(fullPath)
          } else {
            files.push(fullPath)
          }
        }
      } catch {}
    }

    await scan(rootPath)
    return files
  })
}
