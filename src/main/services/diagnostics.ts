import { app, shell } from 'electron'
import fs from 'fs'
import path from 'path'
import type { DiagnosticEntry, DiagnosticsSnapshot, OperationResult } from '../../shared/types/ipc'

const MAX_ENTRIES = 200

function toOperationFailure(error: unknown, fallback: string): OperationResult {
  return {
    success: false,
    error: fallback,
    details: error instanceof Error ? error.message : String(error),
  }
}

export class DiagnosticsService {
  private ensureLogDirectory() {
    fs.mkdirSync(this.getLogDirectory(), { recursive: true })
  }

  public getUserDataPath() {
    return app.getPath('userData')
  }

  public getLogDirectory() {
    return path.join(this.getUserDataPath(), 'logs')
  }

  public getLogFilePath() {
    return path.join(this.getLogDirectory(), 'diagnostics.jsonl')
  }

  public write(level: DiagnosticEntry['level'], source: string, message: string, details?: string) {
    try {
      this.ensureLogDirectory()
      const entry: DiagnosticEntry = {
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        timestamp: new Date().toISOString(),
        level,
        source,
        message,
        details,
      }
      fs.appendFileSync(this.getLogFilePath(), `${JSON.stringify(entry)}\n`, 'utf8')
    } catch {
      // Avoid cascading failures if diagnostics storage is unavailable.
    }
  }

  public info(source: string, message: string, details?: string) {
    this.write('info', source, message, details)
  }

  public warn(source: string, message: string, details?: string) {
    this.write('warning', source, message, details)
  }

  public error(source: string, message: string, details?: string) {
    this.write('error', source, message, details)
  }

  public getSnapshot(): DiagnosticsSnapshot {
    this.ensureLogDirectory()

    let entries: DiagnosticEntry[] = []
    try {
      const raw = fs.readFileSync(this.getLogFilePath(), 'utf8')
      entries = raw
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => JSON.parse(line) as DiagnosticEntry)
        .slice(-MAX_ENTRIES)
        .reverse()
    } catch {
      entries = []
    }

    return {
      generatedAt: new Date().toISOString(),
      userDataPath: this.getUserDataPath(),
      logFilePath: this.getLogFilePath(),
      entries,
    }
  }

  public clear(): OperationResult {
    try {
      this.ensureLogDirectory()
      fs.writeFileSync(this.getLogFilePath(), '', 'utf8')
      return { success: true }
    } catch (error) {
      return toOperationFailure(error, 'Unable to clear diagnostics logs')
    }
  }

  public async openLogFolder(): Promise<OperationResult> {
    try {
      this.ensureLogDirectory()
      const result = await shell.openPath(this.getLogDirectory())
      if (result) {
        return { success: false, error: 'Unable to open the diagnostics folder', details: result }
      }
      return { success: true }
    } catch (error) {
      return toOperationFailure(error, 'Unable to open the diagnostics folder')
    }
  }
}

export const diagnosticsService = new DiagnosticsService()
