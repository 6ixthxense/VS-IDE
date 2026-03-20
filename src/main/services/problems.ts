import { spawn } from 'child_process'
import fs from 'fs'
import path from 'path'
import type { ProblemEntry, ProblemScanResult, ProblemSeverity } from '../../shared/types/ipc'

interface ProcessResult {
  stdout: string
  stderr: string
  exitCode: number | null
}

interface EslintJsonMessage {
  ruleId: string | null
  severity: number
  message: string
  line?: number
  column?: number
}

interface EslintJsonEntry {
  filePath: string
  messages: EslintJsonMessage[]
}

function createProblemId(
  source: ProblemEntry['source'],
  filePath: string,
  line: number,
  column: number,
  message: string,
  code?: string
) {
  return `${source}:${filePath}:${line}:${column}:${code ?? message}`
}

function resolveWorkspaceBinary(rootPath: string, name: string) {
  const extension = process.platform === 'win32' ? '.cmd' : ''
  const binaryPath = path.join(rootPath, 'node_modules', '.bin', `${name}${extension}`)
  return fs.existsSync(binaryPath) ? binaryPath : null
}

function resolveProblemFilePath(rootPath: string, rawPath: string) {
  return path.resolve(rootPath, rawPath.trim())
}

function normalizeSeverity(value: string | number): ProblemSeverity {
  if (value === 'warning' || value === 1) {
    return 'warning'
  }

  return 'error'
}

function sortProblems(entries: ProblemEntry[]) {
  return [...entries].sort((left, right) =>
    left.filePath.localeCompare(right.filePath) ||
    left.line - right.line ||
    left.column - right.column ||
    Number(left.severity === 'warning') - Number(right.severity === 'warning') ||
    left.source.localeCompare(right.source)
  )
}

function dedupeProblems(entries: ProblemEntry[]) {
  const seen = new Set<string>()
  return entries.filter((entry) => {
    if (seen.has(entry.id)) {
      return false
    }

    seen.add(entry.id)
    return true
  })
}

async function runProcess(command: string, args: string[], cwd: string): Promise<ProcessResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      shell: process.platform === 'win32',
      windowsHide: true,
      env: process.env,
    })

    let stdout = ''
    let stderr = ''

    child.stdout.on('data', (chunk) => {
      stdout += chunk.toString()
    })

    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString()
    })

    child.on('error', reject)
    child.on('close', (exitCode) => resolve({ stdout, stderr, exitCode }))
  })
}

export function parseTypeScriptProblems(output: string, rootPath: string): ProblemEntry[] {
  return output
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .flatMap<ProblemEntry>((line) => {
      const match = line.match(/^(.+)\((\d+),(\d+)\): (error|warning) (TS\d+): (.+)$/i)
      if (!match) {
        return []
      }

      const [, rawFilePath, rawLine, rawColumn, rawSeverity, code, message] = match
      const filePath = resolveProblemFilePath(rootPath, rawFilePath)
      const lineNumber = Number.parseInt(rawLine, 10)
      const column = Number.parseInt(rawColumn, 10)
      const severity = normalizeSeverity(rawSeverity)

      return [{
        id: createProblemId('typescript', filePath, lineNumber, column, message, code),
        filePath,
        line: lineNumber,
        column,
        message,
        source: 'typescript',
        severity,
        code,
      }]
    })
}

export function parseEslintProblems(output: string, rootPath: string): ProblemEntry[] {
  const trimmedOutput = output.trim()
  if (!trimmedOutput) {
    return []
  }

  let payload: EslintJsonEntry[]

  try {
    payload = JSON.parse(trimmedOutput) as EslintJsonEntry[]
  } catch {
    return []
  }

  return payload.flatMap<ProblemEntry>((entry) => {
    const filePath = path.isAbsolute(entry.filePath)
      ? entry.filePath
      : resolveProblemFilePath(rootPath, entry.filePath)

    return entry.messages
      .filter((message) => message.severity > 0)
      .map((message) => {
        const line = Math.max(1, Number(message.line ?? 1))
        const column = Math.max(1, Number(message.column ?? 1))
        const severity = normalizeSeverity(message.severity)
        const code = message.ruleId ?? undefined

        return {
          id: createProblemId('eslint', filePath, line, column, message.message, code),
          filePath,
          line,
          column,
          message: message.message,
          source: 'eslint',
          severity,
          code,
        }
      })
  })
}

function createEmptyProblemResult(rootPath: string): ProblemScanResult {
  return {
    rootPath,
    scannedAt: new Date().toISOString(),
    entries: [],
    notes: [],
  }
}

export class ProblemsService {
  private readonly lastResults = new Map<string, ProblemScanResult>()

  public getLastResult(rootPath: string): ProblemScanResult {
    return this.lastResults.get(rootPath) ?? createEmptyProblemResult(rootPath)
  }

  public async scan(rootPath: string): Promise<ProblemScanResult> {
    const notes: string[] = []
    const collectedProblems: ProblemEntry[] = []

    try {
      const tscBinary = resolveWorkspaceBinary(rootPath, 'tsc')
      if (tscBinary) {
        const tscResult = await runProcess(tscBinary, ['--noEmit', '--pretty', 'false'], rootPath)
        collectedProblems.push(...parseTypeScriptProblems(`${tscResult.stdout}\n${tscResult.stderr}`, rootPath))

        if ((tscResult.exitCode ?? 0) > 1 && collectedProblems.length === 0) {
          notes.push('TypeScript scan could not finish cleanly. Check tsconfig.json and local TypeScript dependencies.')
        }
      } else {
        notes.push('Skipped TypeScript scan because this workspace has no local "tsc" binary.')
      }

      const eslintBinary = resolveWorkspaceBinary(rootPath, 'eslint')
      if (eslintBinary) {
        const eslintResult = await runProcess(eslintBinary, ['.', '--format', 'json'], rootPath)
        const eslintProblems = parseEslintProblems(eslintResult.stdout, rootPath)
        collectedProblems.push(...eslintProblems)

        if ((eslintResult.exitCode ?? 0) > 1 && eslintProblems.length === 0) {
          notes.push('ESLint scan could not finish cleanly. Check eslint config and local dependencies.')
        }
      } else {
        notes.push('Skipped ESLint scan because this workspace has no local "eslint" binary.')
      }

      const result: ProblemScanResult = {
        rootPath,
        scannedAt: new Date().toISOString(),
        entries: sortProblems(dedupeProblems(collectedProblems)),
        notes,
      }

      this.lastResults.set(rootPath, result)
      return result
    } catch (error) {
      const result: ProblemScanResult = {
        rootPath,
        scannedAt: new Date().toISOString(),
        entries: sortProblems(dedupeProblems(collectedProblems)),
        notes,
        error: error instanceof Error ? error.message : String(error),
      }

      this.lastResults.set(rootPath, result)
      return result
    }
  }
}

export const problemsService = new ProblemsService()
