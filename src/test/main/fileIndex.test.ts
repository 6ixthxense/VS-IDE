import fs from 'fs'
import os from 'os'
import path from 'path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { workspaceFileIndex } from '../../main/services/fileIndex'

describe('workspaceFileIndex', () => {
  let rootPath = ''

  beforeEach(() => {
    rootPath = fs.mkdtempSync(path.join(os.tmpdir(), 'vs-monitor-ide-index-'))
    fs.mkdirSync(path.join(rootPath, 'src'))
    fs.mkdirSync(path.join(rootPath, 'node_modules'))
    fs.writeFileSync(path.join(rootPath, 'src', 'app.ts'), 'export const app = true\n', 'utf-8')
    fs.writeFileSync(path.join(rootPath, 'README.md'), '# test\n', 'utf-8')
    fs.writeFileSync(path.join(rootPath, 'node_modules', 'skip.js'), 'module.exports = {}\n', 'utf-8')
    workspaceFileIndex.setRootPath(null)
  })

  afterEach(() => {
    workspaceFileIndex.setRootPath(null)
    fs.rmSync(rootPath, { recursive: true, force: true })
  })

  it('reuses the cached index until a workspace change is invalidated', async () => {
    const firstPass = await workspaceFileIndex.getFiles(rootPath)
    expect(firstPass.map((filePath) => path.basename(filePath)).sort()).toEqual(['README.md', 'app.ts'])

    fs.unlinkSync(path.join(rootPath, 'README.md'))

    const cachedPass = await workspaceFileIndex.getFiles(rootPath)
    expect(cachedPass.map((filePath) => path.basename(filePath)).sort()).toEqual(['README.md', 'app.ts'])

    workspaceFileIndex.invalidate(path.join(rootPath, 'README.md'))

    const rebuiltPass = await workspaceFileIndex.getFiles(rootPath)
    expect(rebuiltPass.map((filePath) => path.basename(filePath))).toEqual(['app.ts'])
  })

  it('applies incremental add, rename, and remove updates without rebuilding the whole index', async () => {
    await workspaceFileIndex.getFiles(rootPath)

    const createdPath = path.join(rootPath, 'src', 'notes.ts')
    fs.writeFileSync(createdPath, 'export const notes = true\n', 'utf-8')
    workspaceFileIndex.addFile(createdPath)

    expect(await workspaceFileIndex.getFiles(rootPath)).toContain(createdPath)

    const renamedPath = path.join(rootPath, 'src', 'notes-renamed.ts')
    fs.renameSync(createdPath, renamedPath)
    workspaceFileIndex.renamePath(createdPath, renamedPath)

    const renamedFiles = await workspaceFileIndex.getFiles(rootPath)
    expect(renamedFiles).toContain(renamedPath)
    expect(renamedFiles).not.toContain(createdPath)

    fs.unlinkSync(renamedPath)
    workspaceFileIndex.removePath(renamedPath)

    expect(await workspaceFileIndex.getFiles(rootPath)).not.toContain(renamedPath)
  })
})
