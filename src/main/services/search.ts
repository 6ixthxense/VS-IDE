import fs from 'fs'
import path from 'path'
import type { SearchOptions, SearchResult } from '@shared/types/ipc'
import { workspaceFileIndex } from './fileIndex'

const MAX_RESULTS = 200

const DEFAULT_SEARCH_OPTIONS: SearchOptions = {
  caseSensitive: false,
  wholeWord: false,
  useRegex: false,
  includePattern: '',
  excludePattern: '',
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function normalizePathForMatch(value: string) {
  return value.replace(/\\/g, '/')
}

function splitPatterns(value?: string) {
  return (value ?? '')
    .split(/[\n,;]+/)
    .map((pattern) => pattern.trim())
    .filter(Boolean)
}

function globToRegExp(pattern: string) {
  const escaped = pattern
    .replace(/[.+^${}()|[\]\\]/g, '\\$&')
    .replace(/\*/g, '.*')
    .replace(/\?/g, '.')

  return new RegExp(`^${escaped}$`, 'i')
}

export function normalizeSearchOptions(options?: Partial<SearchOptions>): SearchOptions {
  return {
    caseSensitive: options?.caseSensitive ?? DEFAULT_SEARCH_OPTIONS.caseSensitive,
    wholeWord: options?.wholeWord ?? DEFAULT_SEARCH_OPTIONS.wholeWord,
    useRegex: options?.useRegex ?? DEFAULT_SEARCH_OPTIONS.useRegex,
    includePattern: options?.includePattern?.trim() ?? '',
    excludePattern: options?.excludePattern?.trim() ?? '',
  }
}

export function buildSearchRegExp(query: string, options?: Partial<SearchOptions>) {
  const normalizedOptions = normalizeSearchOptions(options)
  const source = normalizedOptions.useRegex ? query : escapeRegExp(query)
  const wholeWordSource = normalizedOptions.wholeWord ? `\\b(?:${source})\\b` : source
  const flags = normalizedOptions.caseSensitive ? 'g' : 'gi'

  return new RegExp(wholeWordSource, flags)
}

function matchesPatterns(relativePath: string, patternsText?: string) {
  const patterns = splitPatterns(patternsText)
  if (patterns.length === 0) return true

  return patterns.some((pattern) => globToRegExp(normalizePathForMatch(pattern)).test(relativePath))
}

export function shouldSearchPath(relativePath: string, options?: Partial<SearchOptions>) {
  const normalizedPath = normalizePathForMatch(relativePath)
  const normalizedOptions = normalizeSearchOptions(options)

  const included = matchesPatterns(normalizedPath, normalizedOptions.includePattern)
  if (!included) return false

  const excludePatterns = splitPatterns(normalizedOptions.excludePattern)
  if (excludePatterns.length === 0) return true

  return !excludePatterns.some((pattern) => globToRegExp(normalizePathForMatch(pattern)).test(normalizedPath))
}

function createLineMatcher(searchRegExp: RegExp) {
  return (line: string) => new RegExp(searchRegExp.source, searchRegExp.flags).test(line)
}

async function getCandidateFiles(rootPath: string, options: SearchOptions, targetPath?: string) {
  if (targetPath) {
    const stats = await fs.promises.stat(targetPath)

    if (stats.isFile()) {
      const relativePath = normalizePathForMatch(path.relative(rootPath, targetPath))
      return shouldSearchPath(relativePath, options) ? [targetPath] : []
    }

    const indexedFiles = await workspaceFileIndex.getFiles(rootPath)
    const relativeDirectory = normalizePathForMatch(path.relative(rootPath, targetPath)).replace(/\/$/, '')

    if (!relativeDirectory) {
      return indexedFiles.filter((filePath) => {
        const relativePath = normalizePathForMatch(path.relative(rootPath, filePath))
        return shouldSearchPath(relativePath, options)
      })
    }

    const directoryPrefix = `${relativeDirectory}/`
    return indexedFiles.filter((filePath) => {
      const relativePath = normalizePathForMatch(path.relative(rootPath, filePath))
      return relativePath.startsWith(directoryPrefix) && shouldSearchPath(relativePath, options)
    })
  }

  const indexedFiles = await workspaceFileIndex.getFiles(rootPath)
  return indexedFiles.filter((filePath) => {
    const relativePath = normalizePathForMatch(path.relative(rootPath, filePath))
    return shouldSearchPath(relativePath, options)
  })
}

async function collectWorkspaceMatches(
  filePaths: string[],
  searchRegExp: RegExp,
  results: SearchResult[]
): Promise<void> {
  const lineMatches = createLineMatcher(searchRegExp)

  for (const fullPath of filePaths) {
    if (results.length >= MAX_RESULTS) return

    try {
      const content = await fs.promises.readFile(fullPath, 'utf-8')
      if (!new RegExp(searchRegExp.source, searchRegExp.flags).test(content)) continue

      const lines = content.split('\n')
      const name = path.basename(fullPath)

      for (let index = 0; index < lines.length; index += 1) {
        if (lineMatches(lines[index])) {
          results.push({
            path: fullPath,
            name,
            line: index + 1,
            match: lines[index].trim(),
          })
        }

        if (results.length >= MAX_RESULTS) return
      }
    } catch {
      continue
    }
  }
}

export async function searchWorkspace(rootPath: string, query: string, options?: Partial<SearchOptions>) {
  if (!query.trim()) return []

  const normalizedOptions = normalizeSearchOptions(options)
  const searchRegExp = buildSearchRegExp(query, normalizedOptions)
  const results: SearchResult[] = []
  const candidateFiles = await getCandidateFiles(rootPath, normalizedOptions)

  await collectWorkspaceMatches(candidateFiles, searchRegExp, results)
  return results
}

async function replaceInFile(filePath: string, searchRegExp: RegExp, replacement: string) {
  const content = await fs.promises.readFile(filePath, 'utf-8')
  if (!new RegExp(searchRegExp.source, searchRegExp.flags).test(content)) return false

  const nextContent = content.replace(searchRegExp, () => replacement)
  if (nextContent === content) return false

  await fs.promises.writeFile(filePath, nextContent, 'utf-8')
  return true
}

async function replaceInDirectory(filePaths: string[], searchRegExp: RegExp, replacement: string) {
  let changedFiles = 0

  for (const fullPath of filePaths) {
    try {
      if (await replaceInFile(fullPath, searchRegExp, replacement)) {
        changedFiles += 1
      }
    } catch {
      continue
    }
  }

  return changedFiles
}

export async function replaceInWorkspace(
  rootPath: string,
  query: string,
  replacement: string,
  options?: Partial<SearchOptions>,
  targetPath?: string
) {
  if (!query.trim()) {
    return { success: false, count: 0, error: 'Invalid parameters' }
  }

  try {
    const normalizedOptions = normalizeSearchOptions(options)
    const searchRegExp = buildSearchRegExp(query, normalizedOptions)

    if (targetPath) {
      const candidateFiles = await getCandidateFiles(rootPath, normalizedOptions, targetPath)
      const count = await replaceInDirectory(candidateFiles, searchRegExp, replacement)

      return { success: true, count }
    }

    const candidateFiles = await getCandidateFiles(rootPath, normalizedOptions)
    const count = await replaceInDirectory(candidateFiles, searchRegExp, replacement)
    return { success: true, count }
  } catch (error) {
    return { success: false, count: 0, error: (error as Error).message }
  }
}
