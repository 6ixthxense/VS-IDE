import fs from 'fs'
import simpleGit from 'simple-git'
import type { GitDiffRequest, GitStatus, OperationResult } from '../../shared/types/ipc'
import path from 'path'

function operationSuccess(): OperationResult {
  return { success: true }
}

function operationFailure(error: unknown, fallback: string): OperationResult {
  const message = error instanceof Error ? error.message : String(error)
  return {
    success: false,
    error: fallback,
    details: message,
  }
}

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

export async function gitAdd(rootPath: string, filePaths: string[]): Promise<OperationResult> {
  try {
    const git = simpleGit(rootPath)
    await git.add(filePaths)
    return operationSuccess()
  } catch (e) {
    console.error('Git add error:', e)
    return operationFailure(e, 'Unable to stage the selected changes')
  }
}

export async function gitUnstage(rootPath: string, filePaths: string[]): Promise<OperationResult> {
  try {
    const git = simpleGit(rootPath)
    await git.reset(['HEAD', '--', ...filePaths])
    return operationSuccess()
  } catch (e) {
    console.error('Git unstage error:', e)
    return operationFailure(e, 'Unable to unstage the selected changes')
  }
}

export async function gitDiscard(rootPath: string, request: GitDiffRequest): Promise<OperationResult> {
  try {
    if (request.status === 'renamed') {
      return { success: false, error: 'Discard for renamed files is not supported yet' }
    }

    if (request.status === 'untracked' || request.status === 'added') {
      await fs.promises.rm(request.filePath, { recursive: true, force: true })
      return operationSuccess()
    }

    const git = simpleGit(rootPath)
    const relativePath = path.relative(rootPath, request.filePath)
    const args = ['restore', '--source=HEAD']

    if (request.staged) {
      args.push('--staged')
    }

    args.push('--worktree', '--', relativePath)
    await git.raw(args)
    return operationSuccess()
  } catch (e) {
    console.error('Git discard error:', e)
    return operationFailure(e, 'Unable to discard local changes')
  }
}

export async function gitCommit(rootPath: string, message: string): Promise<OperationResult> {
  try {
    const git = simpleGit(rootPath)
    await git.commit(message)
    return operationSuccess()
  } catch (e) {
    console.error('Git commit error:', e)
    return operationFailure(e, 'Commit failed')
  }
}

export async function gitPush(rootPath: string): Promise<OperationResult> {
  try {
    const git = simpleGit(rootPath)
    await git.push()
    return operationSuccess()
  } catch (e) {
    console.error('Git push error:', e)
    return operationFailure(e, 'Push failed')
  }
}

export async function gitPull(rootPath: string): Promise<OperationResult> {
  try {
    const git = simpleGit(rootPath)
    await git.pull()
    return operationSuccess()
  } catch (e) {
    console.error('Git pull error:', e)
    return operationFailure(e, 'Pull failed')
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
