import fs from 'fs'
import simpleGit from 'simple-git'
import type { GitDiffRequest, GitStatus } from '../../shared/types/ipc'
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
    const message = (e as Error).message || ''
    if (!message.includes('not a git repository')) {
      console.error('Git status error:', e)
    }
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

export async function gitUnstage(rootPath: string, filePaths: string[]): Promise<boolean> {
  try {
    const git = simpleGit(rootPath)
    await git.reset(['HEAD', '--', ...filePaths])
    return true
  } catch (e) {
    console.error('Git unstage error:', e)
    return false
  }
}

export async function gitDiscard(rootPath: string, request: GitDiffRequest): Promise<boolean> {
  try {
    if (request.status === 'renamed') {
      return false
    }

    if (request.status === 'untracked' || request.status === 'added') {
      await fs.promises.rm(request.filePath, { recursive: true, force: true })
      return true
    }

    const git = simpleGit(rootPath)
    const relativePath = path.relative(rootPath, request.filePath)
    const args = ['restore', '--source=HEAD']

    if (request.staged) {
      args.push('--staged')
    }

    args.push('--worktree', '--', relativePath)
    await git.raw(args)
    return true
  } catch (e) {
    console.error('Git discard error:', e)
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

function buildSyntheticDiff(filePath: string, content: string, status?: string) {
  const normalizedPath = filePath.replace(/\\/g, '/')
  const lines = content.split('\n')
  const prefix = status === 'deleted' ? '-' : '+'

  return [
    `diff --git a/${normalizedPath} b/${normalizedPath}`,
    `--- a/${normalizedPath}`,
    `+++ b/${normalizedPath}`,
    ...lines.map((line) => `${prefix}${line}`),
  ].join('\n')
}

export async function getGitDiff(rootPath: string, request: GitDiffRequest): Promise<string> {
  try {
    const git = simpleGit(rootPath)
    const relativePath = path.relative(rootPath, request.filePath)

    if (request.status === 'untracked') {
      const content = await fs.promises.readFile(request.filePath, 'utf-8')
      return buildSyntheticDiff(relativePath, content, request.status)
    }

    const diff = request.staged
      ? await git.diff(['--cached', '--', relativePath])
      : await git.diff(['--', relativePath])

    if (diff.trim()) {
      return diff
    }

    if (request.status === 'added') {
      const content = await fs.promises.readFile(request.filePath, 'utf-8')
      return buildSyntheticDiff(relativePath, content, request.status)
    }

    return 'No diff available for this change.'
  } catch (e) {
    console.error('Git diff error:', e)
    return 'Unable to load diff preview.'
  }
}
