import simpleGit from 'simple-git'
import type { GitStatus, GitFileStatus } from '../../shared/types/ipc'
import path from 'path'

export async function getGitStatus(rootPath: string): Promise<GitStatus> {
  try {
    const git = simpleGit(rootPath)
    const status = await git.status()
    const result: GitStatus = {}

    // Map simple-git status to our GitFileStatus
    status.created.forEach(f => result[path.join(rootPath, f)] = 'added')
    status.modified.forEach(f => result[path.join(rootPath, f)] = 'modified')
    status.deleted.forEach(f => result[path.join(rootPath, f)] = 'deleted')
    status.renamed.forEach(f => result[path.join(rootPath, f.to)] = 'renamed')
    status.not_added.forEach(f => result[path.join(rootPath, f)] = 'untracked')
    status.staged.forEach(f => result[path.join(rootPath, f)] = 'staged')

    return result
  } catch (e) {
    console.error('Git status error:', e)
    return {}
  }
}

export async function gitAdd(rootPath: string, filePaths: string[]): Promise<boolean> {
  try {
    const git = simpleGit(rootPath)
    await git.add(filePaths)
    return true
  } catch (e) {
    console.error('Git add error:', e)
    return false
  }
}

export async function gitCommit(rootPath: string, message: string): Promise<boolean> {
  try {
    const git = simpleGit(rootPath)
    await git.commit(message)
    return true
  } catch (e) {
    console.error('Git commit error:', e)
    return false
  }
}

export async function gitPush(rootPath: string): Promise<boolean> {
  try {
    const git = simpleGit(rootPath)
    await git.push()
    return true
  } catch (e) {
    console.error('Git push error:', e)
    return false
  }
}

export async function gitPull(rootPath: string): Promise<boolean> {
  try {
    const git = simpleGit(rootPath)
    await git.pull()
    return true
  } catch (e) {
    console.error('Git pull error:', e)
    return false
  }
}
